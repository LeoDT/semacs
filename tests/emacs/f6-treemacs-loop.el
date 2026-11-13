;;; f6-treemacs-loop.el --- treemacs reference in a real command loop -*- lexical-binding: t; -*-
;;
;; Loaded by f6-drive.py through a pty (`emacs -Q -nw`). Unlike f6-treemacs.el
;; this runs redisplay and idle timers, which is what
;; `window-configuration-change-hook' (side window parameters) and
;; `treemacs-follow-mode' need.
;;
;; Environment: EMACS_REFERENCE_STATE (jsonl dump file),
;; EMACS_REFERENCE_ROOT (fixture root created by the driver).
;; `C-c d' writes one dump line; the driver sends it after every step.

(require 'json)

(let ((autoloads (expand-file-name
                  "treemacs-autoloads.el"
                  (file-name-directory (locate-library "treemacs")))))
  (load autoloads nil 'nomessage))
(require 'treemacs)

(defvar reference/state-file (getenv "EMACS_REFERENCE_STATE"))
(defvar reference/root (getenv "EMACS_REFERENCE_ROOT"))

;; Keep persistence files out of the fixture (they would show up as tree nodes)
;; and out of the user's real ~/.emacs.d/.cache; the driver points this at
;; .pi/tmp/emacs-reference/.
(setq treemacs-persist-file (getenv "EMACS_REFERENCE_PERSIST")
      treemacs-last-error-persist-file (concat (getenv "EMACS_REFERENCE_PERSIST") "-error")
      inhibit-startup-screen t)

(defun reference/mk (relative content)
  (let ((path (expand-file-name relative reference/root)))
    (make-directory (file-name-directory path) t)
    (with-temp-file path (insert content))
    path))

(defun reference/window-state ()
  (vconcat
   (mapcar (lambda (window)
             (list :buffer (buffer-name (window-buffer window))
                   :side (window-parameter window 'window-side)
                   :slot (window-parameter window 'window-slot)
                   :no-delete (window-parameter window 'no-delete-other-windows)
                   :width (window-total-width window)
                   :selected (eq window (selected-window))))
           (window-list nil 'no-minibuf))))

(defun reference/dump ()
  (interactive)
  (let* ((tree (treemacs-get-local-buffer))
         (line (when tree
                 (with-current-buffer tree
                   (buffer-substring-no-properties
                    (line-beginning-position) (line-end-position)))))
         (record (list :tag this-command
                       :selected (buffer-name)
                       :visibility (treemacs-current-visibility)
                       :windows (reference/window-state)
                       :tree (when (buffer-live-p tree)
                               (with-current-buffer tree
                                 (buffer-substring-no-properties (point-min) (point-max))))
                       :tree-line line
                       :mode (format "%s %s follow=%s git=%s gitignore=%s"
                                     major-mode
                                     (if buffer-file-name "file" "nofile")
                                     treemacs-follow-mode
                                     treemacs-git-mode
                                     treemacs-hide-gitignored-files-mode))))
    (with-temp-buffer
      (insert (json-encode record) "\n")
      (write-region (point-min) (point-max) reference/state-file t 'silent))
    (message "dumped")))

(defun reference/setup ()
  ;; Fixture: a repository with one plain file, one nested directory, one
  ;; dotfile and one git-ignored file.
  (when (executable-find "git")
    (let ((default-directory (file-name-as-directory reference/root)))
      (process-file "git" nil nil nil "init" "--quiet")))
  (reference/mk ".gitignore" "ignored.log\n")
  (reference/mk "aaa.txt" "aaa\n")
  (reference/mk ".hidden.txt" "hidden\n")
  (reference/mk "ignored.log" "ignored\n")
  (reference/mk "b-dir/inner.txt" "inner\n")
  (setq default-directory (file-name-as-directory reference/root))
  (treemacs--maybe-load-workspaces)
  (treemacs-do-add-project-to-workspace
   (treemacs-canonical-path reference/root) (treemacs--filename reference/root))
  (switch-to-buffer (find-file-noselect (expand-file-name "aaa.txt" reference/root)))
  (treemacs--init)
  (global-set-key (kbd "C-c d") #'reference/dump)
  (reference/dump)
  (message "ready"))

(add-hook 'window-setup-hook #'reference/setup)
