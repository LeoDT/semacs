;;; m2-files.el --- Emacs reference for M2 file / buffer behavior -*- lexical-binding: t; -*-
;;
;; Direct function / interactive-spec reference for the file and buffer rules
;; implemented by semacs. Run with:
;;
;;   emacs -Q --batch -l tests/emacs/m2-files.el -f ert-run-tests-batch-and-exit
;;
;; This is external evidence, not a semacs pass record. Prompts are captured by
;; stubbing only the user query; the surrounding Emacs code path is real.
;; Real command-loop exit behavior is recorded separately (see RESULTS.md).

(require 'cl-lib)
(require 'ert)

(defun reference/capture (thunk)
  "Run THUNK with y-or-n-p answering no and capture that prompt."
  (let (prompt)
    (cl-letf (((symbol-function 'y-or-n-p)
               (lambda (p) (setq prompt p) nil)))
      (funcall thunk))
    prompt))

(ert-deftest m2-file-buffer-names-keep-the-first-and-suffix-the-new-one ()
  (let ((first (create-file-buffer "/tmp/semacs-reference-one/foo"))
        (second (create-file-buffer "/tmp/semacs-reference-two/foo")))
    (unwind-protect
        (progn
          (should (equal (buffer-name first) "foo"))
          (should (equal (buffer-name second) "foo<semacs-reference-two>")))
      (kill-buffer first)
      (kill-buffer second))))

(ert-deftest m2-find-file-reuses-one-buffer-for-a-file-object ()
  (let* ((dir (make-temp-file "semacs-m2-identity" t))
         (real (expand-file-name "real.txt" dir))
         (link (expand-file-name "link.txt" dir))
         (dotdot (expand-file-name
                  (concat (file-name-nondirectory dir) "/../"
                          (file-name-nondirectory dir) "/real.txt")
                  (file-name-directory (directory-file-name dir))))
         (buffers nil))
    (unwind-protect
        (progn
          (with-temp-file real (insert "base"))
          (make-symbolic-link "real.txt" link)
          (setq buffers (list (find-file-noselect real)
                              (find-file-noselect link)
                              (find-file-noselect dotdot)))
          (should (eq (nth 0 buffers) (nth 1 buffers)))
          (should (eq (nth 0 buffers) (nth 2 buffers)))
          (should (equal (buffer-name (nth 0 buffers)) "real.txt")))
      (dolist (buffer buffers)
        (when (buffer-live-p buffer) (kill-buffer buffer)))
      (delete-directory dir t))))

(ert-deftest m2-saving-a-deleted-file-recreates-it-without-a-prompt ()
  (let* ((dir (make-temp-file "semacs-m2-deleted" t))
         (path (expand-file-name "file.txt" dir))
         (asked nil)
         (buffer nil))
    (unwind-protect
        (progn
          (with-temp-file path (insert "base"))
          (setq buffer (find-file-noselect path))
          (with-current-buffer buffer
            (goto-char (point-max))
            (insert "X")
            (delete-file path)
            (cl-letf (((symbol-function 'yes-or-no-p)
                       (lambda (prompt) (setq asked prompt) t)))
              (save-buffer))
            (should-not asked)
            (should-not (buffer-modified-p))
            ;; Emacs' default final-newline handling adds the newline here;
            ;; semacs keeps the original file's final-newline state instead.
            (should (equal (buffer-string) "baseX\n")))
          (should (equal (with-temp-buffer
                           (insert-file-contents path)
                           (buffer-string))
                         "baseX\n")))
      (when (buffer-live-p buffer) (kill-buffer buffer))
      (delete-directory dir t))))

(ert-deftest m2-write-file-overwrite-query-is-y-or-n-p ()
  (let* ((dir (make-temp-file "semacs-m2-overwrite" t))
         (source (expand-file-name "source.txt" dir))
         (target (expand-file-name "target.txt" dir))
         (prompt nil)
         (buffer nil))
    (unwind-protect
        (progn
          (with-temp-file source (insert "base"))
          (with-temp-file target (insert "old"))
          (setq buffer (find-file-noselect source))
          (with-current-buffer buffer
            (goto-char (point-max))
            (insert "X")
            (setq prompt
                  (reference/capture
                   (lambda ()
                     (cl-letf (((symbol-function 'read-file-name)
                                (lambda (&rest _) target)))
                       (should-error (call-interactively 'write-file)
                                     :type 'user-error))))))
          (should (string-match-p "exists; overwrite?" prompt))
          ;; y-or-n-p appends the "(y or n)" suffix itself; the captured
          ;; argument is only the question. semacs stores the suffix in the
          ;; prompt because its minibuffer read session has no such helper.
          (should-not (string-match-p "(y or n)" prompt)))
      (when (buffer-live-p buffer)
        (with-current-buffer buffer (set-buffer-modified-p nil))
        (kill-buffer buffer))
      (delete-directory dir t))))

(ert-deftest m2-kill-buffer-reads-a-name-defaulting-to-the-current-buffer ()
  (let ((current (get-buffer-create "semacs-reference-current"))
        (other (get-buffer-create "semacs-reference-other"))
        (captured nil))
    (unwind-protect
        (progn
          (with-current-buffer current
            (cl-letf (((symbol-function 'completing-read-default)
                       (lambda (prompt _collection &optional _predicate require-match
                                       _initial _hist default _inherit)
                         (setq captured (list prompt require-match default))
                         "semacs-reference-other")))
              (call-interactively 'kill-buffer)))
          (should (equal (nth 0 captured)
                         "Kill buffer (default semacs-reference-current): "))
          (should (nth 1 captured))
          (should (equal (nth 2 captured) "semacs-reference-current"))
          (should-not (buffer-live-p other))
          (should (buffer-live-p current)))
      (when (buffer-live-p other) (kill-buffer other))
      (when (buffer-live-p current) (kill-buffer current)))))

(ert-deftest m2-switch-to-buffer-excludes-current-from-default-and-candidates ()
  (let ((current (get-buffer-create "semacs-reference-a"))
        (other (get-buffer-create "semacs-reference-b"))
        (captured nil))
    (unwind-protect
        (progn
          (switch-to-buffer other)
          (switch-to-buffer current)
          (cl-letf (((symbol-function 'read-buffer)
                     (lambda (prompt &optional default require-match &rest _)
                       (setq captured
                             (list prompt
                                   (if (bufferp default) (buffer-name default) default)
                                   require-match))
                       "semacs-reference-b")))
            (should (equal (read-buffer-to-switch "Switch to buffer")
                           "semacs-reference-b")))
          (should (equal (nth 0 captured) "Switch to buffer"))
          (should (equal (nth 1 captured) "semacs-reference-b"))
          (should (eq (nth 2 captured) 'confirm-after-completion)))
      (when (buffer-live-p other) (kill-buffer other))
      (when (buffer-live-p current) (kill-buffer current)))))
