;;; f5-magit.el --- Magit commit/history reference -*- lexical-binding: t; -*-

(require 'ert)
(require 'cl-lib)
(require 'magit)
(require 'magit-commit)

(defconst f5-reference-directory
  (expand-file-name "../../.pi/tmp/" (file-name-directory load-file-name)))

(defun f5/git (&rest args)
  (with-temp-buffer
    (let ((status (apply #'process-file "git" nil (current-buffer) nil args)))
      (unless (zerop status)
        (error "git %S failed (%s): %s" args status (buffer-string))))
    (string-trim-right (buffer-string))))

(defmacro f5/with-repository (&rest body)
  (declare (indent 0))
  `(progn
     (make-directory f5-reference-directory t)
     (let* ((directory (make-temp-file
                        (expand-file-name "f5-magit-" f5-reference-directory) t))
            (default-directory (file-name-as-directory directory))
            (process-environment (copy-sequence process-environment))
            (initial-buffers (buffer-list))
            (magit-git-environment nil)
            (magit-published-branches '("origin/master"))
            (magit-commit-ask-to-stage 'verbose)
            (magit-commit-squash-confirm t)
            (magit-commit-show-diff t)
            (current-prefix-arg nil))
       (dolist (variable '("GIT_DIR" "GIT_WORK_TREE" "GIT_INDEX_FILE"
                           "GIT_COMMON_DIR" "GIT_OBJECT_DIRECTORY"
                           "GIT_ALTERNATE_OBJECT_DIRECTORIES" "GIT_CONFIG_COUNT"
                           "GIT_CONFIG_PARAMETERS" "GIT_CONFIG" "GIT_NAMESPACE"
                           "GIT_SHALLOW_FILE" "GIT_REPLACE_REF_BASE"
                           "GIT_AUTHOR_NAME" "GIT_AUTHOR_EMAIL"
                           "GIT_COMMITTER_NAME" "GIT_COMMITTER_EMAIL"
                           "GIT_AUTHOR_DATE" "GIT_COMMITTER_DATE"))
         (setenv variable nil))
       (setenv "GIT_CONFIG_NOSYSTEM" "1")
       (setenv "GIT_CONFIG_GLOBAL" "/dev/null")
       (unwind-protect
           (progn
             (f5/git "-c" "init.templateDir=" "init" "--initial-branch=main")
             (f5/git "config" "user.name" "Reference")
             (f5/git "config" "user.email" "reference@example.invalid")
             (f5/git "config" "commit.gpgSign" "false")
             (with-temp-file "file.txt" (insert "base\n"))
             (f5/git "add" "file.txt")
             (f5/git "commit" "-m" "base")
             ,@body)
         (dolist (buffer (buffer-list))
           (when (and (buffer-live-p buffer)
                      (not (memq buffer initial-buffers))
                      (with-current-buffer buffer
                        (string-prefix-p directory default-directory)))
             (with-current-buffer buffer (set-buffer-modified-p nil))
             (kill-buffer buffer)))
         (delete-directory directory t)))))

(defun f5/stage-change ()
  (with-temp-file "file.txt" (insert "staged\n"))
  (f5/git "add" "file.txt"))

(ert-deftest f5-amend-without-staged-still-starts-editor-command ()
  (f5/with-repository
    (let (started)
      (cl-letf (((symbol-function 'magit-run-git-with-editor)
                 (lambda (&rest args) (setq started (flatten-tree args))))
                ((symbol-function 'y-or-n-p)
                 (lambda (prompt) (error "Unexpected question: %s" prompt))))
        (magit-commit-amend '("--verbose")))
      (should (equal started '("commit" "--amend" "--verbose"))))))

(ert-deftest f5-reword-keeps-head-tree-and-staged-index ()
  (f5/with-repository
    (f5/stage-change)
    (let ((tree (f5/git "rev-parse" "HEAD^{tree}"))
          (index (f5/git "write-tree"))
          (editor (expand-file-name "editor.sh"))
          started)
      (with-temp-file editor
        (insert "#!/bin/sh\nprintf '%s\\n' 'reworded' > \"$1\"\n"))
      (set-file-modes editor #o700)
      (setenv "GIT_EDITOR" (shell-quote-argument editor))
      (cl-letf (((symbol-function 'magit-run-git-with-editor)
                 (lambda (&rest args)
                   (setq started (flatten-tree args))
                   (apply #'f5/git started))))
        (magit-commit-reword nil t))
      (should (equal started '("commit" "--amend" "--only" "--allow-empty")))
      (should (equal (f5/git "log" "-1" "--format=%s") "reworded"))
      (should (equal tree (f5/git "rev-parse" "HEAD^{tree}")))
      (should (equal index (f5/git "write-tree")))
      (should-not (equal tree index)))))

(ert-deftest f5-fixup-and-squash-select-before-start-and-do-not-edit ()
  (dolist (kind '(fixup squash))
    (f5/with-repository
      (f5/stage-change)
      (let ((target (f5/git "rev-parse" "HEAD"))
            select started diff)
        (cl-letf (((symbol-function 'magit-log-select)
                   (lambda (pick &rest _) (setq select pick)))
                  ((symbol-function 'magit-diff-staged)
                   (lambda (&rest _) (setq diff t)))
                  ((symbol-function 'magit-run-git-with-editor)
                   (lambda (&rest args) (setq started (flatten-tree args)))))
          (funcall (if (eq kind 'fixup) #'magit-commit-fixup #'magit-commit-squash)
                   target '("--verbose"))
          (should select)
          (should diff)
          (should-not started)
          (funcall select target)
          (should (equal started
                         (list "commit" "--no-edit"
                               (format "--%s=%s" kind target) "--verbose"))))))))

(ert-deftest f5-fixup-unstaged-only-asks-to-commit-all-before-selection ()
  (f5/with-repository
    (with-temp-file "file.txt" (insert "unstaged\n"))
    (let (questions preview select started)
      (cl-letf (((symbol-function 'y-or-n-p)
                 (lambda (prompt) (push prompt questions) t))
                ((symbol-function 'magit-diff-unstaged)
                 (lambda (&rest _) (setq preview t)))
                ((symbol-function 'magit-diff-staged) #'ignore)
                ((symbol-function 'magit-log-select)
                 (lambda (pick &rest _) (setq select pick)))
                ((symbol-function 'magit-run-git-with-editor)
                 (lambda (&rest args) (setq started (flatten-tree args)))))
        (magit-commit-fixup nil '("--verbose"))
        (should preview)
        (should (equal questions '("Nothing staged.  Commit all uncommitted changes? ")))
        (should select)
        (should-not started)
        (funcall select (f5/git "rev-parse" "HEAD"))
        (should (member "--all" started))
        (should (member "--no-edit" started))))))

(ert-deftest f5-fixup-declining-all-does-not-select-or-start ()
  (f5/with-repository
    (with-temp-file "file.txt" (insert "unstaged\n"))
    (let (preview)
      (cl-letf (((symbol-function 'y-or-n-p) (lambda (_) nil))
                ((symbol-function 'magit-diff-unstaged)
                 (lambda (&rest _) (setq preview t)))
                ((symbol-function 'magit-log-select)
                 (lambda (&rest _) (ert-fail "Unexpected selection")))
                ((symbol-function 'magit-run-git-with-editor)
                 (lambda (&rest _) (ert-fail "Unexpected commit"))))
        (magit-commit-fixup nil '("--verbose"))
        (should preview)))))

(ert-deftest f5-fixup-empty-repository-state-is-rejected ()
  (f5/with-repository
    (should-error (magit-commit-fixup nil '("--verbose")) :type 'user-error)))

(ert-deftest f5-selection-pick-is-not-the-p-navigation-key ()
  (dolist (key '("." "e" "C-c C-c"))
    (should (eq (lookup-key magit-log-select-mode-map (kbd key))
                'magit-log-select-pick)))
  (should-not (eq (lookup-key magit-log-select-mode-map (kbd "p"))
                  'magit-log-select-pick))
  (should (eq (lookup-key magit-log-select-mode-map (kbd "q"))
              'magit-log-select-quit)))

(ert-deftest f5-log-defaults-and-doubling-limit ()
  (should (equal (get 'magit-log-mode 'magit-log-default-arguments)
                 '("--graph" "-n256" "--decorate")))
  (should (equal (get 'magit-log-select-mode 'magit-log-default-arguments)
                 '("--graph" "-n256" "--decorate")))
  (with-temp-buffer
    (setq-local magit-buffer-log-args '("--graph" "-n256" "--decorate"))
    (cl-letf (((symbol-function 'magit-refresh) #'ignore))
      (magit-log-double-commit-limit)
      (should (= (magit-log-get-commit-limit) 512))
      (magit-log-half-commit-limit)
      (should (= (magit-log-get-commit-limit) 256)))))

(ert-deftest f5-log-current-uses-current-branch-or-detached-head ()
  (f5/with-repository
    (let (revisions)
      (cl-letf (((symbol-function 'magit-log-setup-buffer)
                 (lambda (revs &rest _) (setq revisions revs))))
        (magit-log-current)
        (should (equal revisions '("main")))
        (f5/git "checkout" "--detach")
        (magit-log-current)
        (should (equal revisions '("HEAD")))))))

(ert-deftest f5-published-amend-queries-before-starting ()
  (f5/with-repository
    (f5/git "update-ref" "refs/remotes/origin/master" "HEAD")
    (let (question)
      (cl-letf (((symbol-function 'magit-confirm)
                 (lambda (action &rest args)
                   (setq question (cons action args))
                   (user-error "Abort")))
                ((symbol-function 'magit-run-git-with-editor)
                 (lambda (&rest _) (ert-fail "Unexpected commit"))))
        (should-error (magit-commit-amend) :type 'user-error))
      (should (eq (car question) 'amend-published))
      (should (equal (car (last question)) '("origin/master"))))))

(ert-deftest f5-revision-diff-uses-show-not-first-parent-diff ()
  (let ((magit-buffer-diff-args '("--stat" "--no-ext-diff"))
        (magit-buffer-revision "HEAD")
        (magit-buffer-diff-files nil)
        arguments)
    (cl-letf (((symbol-function 'magit--insert-diff)
               (lambda (&rest args) (setq arguments (flatten-tree args))))
              ((symbol-function 'magit--rev-dereference) #'identity))
      (magit-insert-revision-diff))
    (should (equal arguments
                   '(t "show" "-p" "--format=" "--no-prefix" "--numstat"
                       "--stat" "--no-ext-diff" "HEAD" "--")))))

(ert-deftest f5-root-revision-displays-added-content ()
  (f5/with-repository
    (let ((buffer (magit-revision-setup-buffer
                   "HEAD" '("--stat" "--no-ext-diff") nil)))
      (with-current-buffer buffer
        (should (string-match-p (regexp-quote "+base") (buffer-string)))))))

(ert-deftest f5-merge-revision-displays-combined-resolution ()
  (f5/with-repository
    (let ((base (f5/git "rev-parse" "HEAD")))
      (with-temp-file "file.txt" (insert "main\n"))
      (f5/git "add" "file.txt")
      (f5/git "commit" "-m" "main change")
      (let ((main (f5/git "rev-parse" "HEAD")))
        (f5/git "checkout" "-b" "side" base)
        (with-temp-file "file.txt" (insert "side\n"))
        (f5/git "add" "file.txt")
        (f5/git "commit" "-m" "side change")
        (let ((side (f5/git "rev-parse" "HEAD")))
          (f5/git "checkout" "main")
          (with-temp-file "file.txt" (insert "resolved\n"))
          (f5/git "add" "file.txt")
          (let* ((tree (f5/git "write-tree"))
                 (merge (f5/git "commit-tree" tree "-p" main "-p" side "-m" "merge")))
            (f5/git "reset" "--hard" merge)
            (should (string-match-p "diff --cc file.txt"
                                    (f5/git "show" "-p" "--format=" "HEAD")))
            (let ((buffer (magit-revision-setup-buffer
                           "HEAD" '("--stat" "--no-ext-diff") nil)))
              (with-current-buffer buffer
                (should (string-match-p "@@@" (buffer-string)))
                (should (string-match-p (regexp-quote "++resolved")
                                        (buffer-string)))))))))))

(provide 'f5-magit)
;;; f5-magit.el ends here
