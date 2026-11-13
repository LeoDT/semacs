;;; f6-treemacs.el --- treemacs tree viewer reference -*- lexical-binding: t; -*-

;; Reference for the folder tree viewer (plan stage F6). Records what upstream
;; treemacs actually does, so semacs can follow it or register a deviation.
;;
;; Fixed environment (see tests/emacs/README.md for the exact -L list):
;;   GNU Emacs 31.1
;;   treemacs 20251226.1307, upstream commit
;;   2ab5a3c89fa01bbbd99de9b8986908b2bc5a7b49
;;
;; Evidence boundary: batch ERT. `emacs -Q --batch` has a real window tree, but
;; redisplay never runs, so `window-configuration-change-hook' and idle timers
;; (follow, deferred git) do not fire here. Those paths are observed by
;; f6-drive.py in a real `emacs -Q -nw` command loop and are not claimed here.
;; Fixtures live under .pi/tmp/ and are deleted afterwards.

(require 'ert)
(require 'cl-lib)

;; Upstream expects its autoloads to be active; loading only `treemacs' would
;; leave tag/extension entry points undefined and produce errors that do not
;; occur in an installed package.
(let ((autoloads (expand-file-name
                  "treemacs-autoloads.el"
                  (file-name-directory (locate-library "treemacs")))))
  (load autoloads nil 'nomessage))
(require 'treemacs)

(defconst f6-reference-directory
  (expand-file-name "../../.pi/tmp/" (file-name-directory load-file-name)))

;; The no-VCS fixture must not sit inside a repository; .pi/tmp/ is inside this
;; project's own checkout, so that one fixture uses the system temp directory.
(defconst f6-outside-directory temporary-file-directory)

;; Never read or write the user's real persistence files.
(setq treemacs-persist-file
      (expand-file-name "f6-treemacs-persist" f6-reference-directory)
      treemacs-last-error-persist-file
      (expand-file-name "f6-treemacs-persist-error" f6-reference-directory))

;; Captured before any test reconfigures the modes: treemacs turns these on at
;; load time when git and python3 are available.
(defconst f6-load-state
  (list :follow treemacs-follow-mode
        :filewatch treemacs-filewatch-mode
        :fringe treemacs-fringe-indicator-mode
        :git-mode treemacs-git-mode
        :git-variant treemacs--git-mode
        :collapse-dirs treemacs-collapse-dirs
        :python treemacs-python-executable))

;; Keep the remaining tests deterministic and free of async git processes.
(treemacs-follow-mode -1)
(treemacs-filewatch-mode -1)
(treemacs-fringe-indicator-mode -1)
(treemacs-git-mode -1)

(make-directory f6-reference-directory t)

(defun f6/mk (root relative content)
  "Create RELATIVE below ROOT with CONTENT and return its absolute name."
  (let ((path (expand-file-name relative root)))
    (make-directory (file-name-directory path) t)
    (with-temp-file path (insert content))
    path))

(defun f6/tree-text ()
  "Return the current treemacs buffer text."
  (with-current-buffer (treemacs-get-local-buffer)
    (buffer-substring-no-properties (point-min) (point-max))))

(defun f6/tree-line ()
  "Return the line around point in the treemacs buffer."
  (with-current-buffer (treemacs-get-local-buffer)
    (buffer-substring-no-properties (line-beginning-position) (line-end-position))))

(defun f6/goto-node (text)
  "Put point on the tree button whose label contains TEXT."
  (with-current-buffer (treemacs-get-local-buffer)
    (goto-char (point-min))
    (unless (search-forward text nil t)
      (error "No tree node named %s" text))
    (goto-char (match-beginning 0))))

(defun f6/windows ()
  "Describe the visible windows as (BUFFER SIDE PARAM-SELECTED) tuples."
  (mapcar (lambda (window)
            (list (buffer-name (window-buffer window))
                  (window-parameter window 'window-side)
                  (eq window (selected-window))))
          (window-list nil 'no-minibuf)))

(defun f6/tree-window ()
  (get-buffer-window (treemacs-get-local-buffer)))

(defun f6/reset ()
  "Drop the treemacs buffer/window and start from an empty workspace."
  (let ((buffer (treemacs-get-local-buffer)))
    (when (buffer-live-p buffer)
      (let ((window (get-buffer-window buffer)))
        (when window (delete-window window)))
      (kill-buffer buffer)))
  (when (> (length (window-list nil 'no-minibuf)) 1)
    (delete-other-windows))
  (setf treemacs--workspaces (list (treemacs-workspace->create! :name "Default"))
        (treemacs-current-workspace) (car treemacs--workspaces))
  (put 'treemacs :state-is-restored t))

(defmacro f6/with-fixture (&rest body)
  "Build the standard fixture, initialise treemacs on it and run BODY.
BODY sees ROOT (directory kept without trailing slash), CURRENT (buffer of
aaa.txt) and PROJ (the treemacs project)."
  (declare (indent 0))
  `(let* ((root (directory-file-name
                 (make-temp-file (expand-file-name "f6-treemacs-" f6-reference-directory) t)))
          (default-directory (file-name-as-directory root))
          (initial-buffers (buffer-list)))
     (unwind-protect
         (progn
           (make-directory (expand-file-name ".git" root) t)
           (f6/mk root "aaa.txt" "aaa\n")
           (f6/mk root "B-upper.txt" "upper\n")
           (f6/mk root "1.txt" "one\n")
           (f6/mk root "ignored.log" "ignored\n")
           (f6/mk root ".gitignore" "ignored.log\n")
           (f6/mk root ".hidden.txt" "hidden\n")
           (f6/mk root "b-dir/inner.txt" "inner\n")
           (f6/mk root "chain/one/two/deep.txt" "deep\n")
           (f6/mk root "Dir1/y.txt" "y\n")
           (f6/mk root "dir2/x.txt" "x\n")
           (f6/reset)
           (treemacs-do-add-project-to-workspace
            (treemacs-canonical-path root) (treemacs--filename root))
           (let* ((current (find-file-noselect (expand-file-name "aaa.txt" root)))
                  (proj nil))
             (switch-to-buffer current)
             (treemacs--init)
             (setq proj (car (treemacs-workspace->projects (treemacs-current-workspace))))
             (let ((root root) (current current) (proj proj))
               ,@body)))
       (f6/reset)
       (dolist (buffer (buffer-list))
         (when (and (buffer-live-p buffer)
                    (not (memq buffer initial-buffers))
                    (with-current-buffer buffer
                      (string-prefix-p (file-name-as-directory root) default-directory)))
           (with-current-buffer buffer (set-buffer-modified-p nil))
           (kill-buffer buffer)))
       (delete-directory root t))))

;;; Defaults

(ert-deftest f6-default-window-shape ()
  (should (eq 'left treemacs-position))
  (should (equal 35 treemacs-width))
  (should treemacs-display-in-side-window)
  (should treemacs-no-delete-other-windows)
  (should-not treemacs-is-never-other-window)
  (should (equal 2 treemacs-indentation))
  (should (equal " " treemacs-indentation-string)))

(ert-deftest f6-default-tree-options ()
  (should treemacs-show-hidden-files)
  (should treemacs-hide-dot-git-directory)
  (should treemacs-hide-dot-jj-directory)
  (should-not treemacs-hide-gitignored-files-mode)
  (should (eq 'treemacs-visit-node-no-split treemacs-default-visit-action))
  (should-not treemacs-follow-after-init)
  (should treemacs-expand-after-init)
  (should (eq 'move-back treemacs-select-when-already-in-treemacs))
  (should treemacs-wrap-around)
  (should (eq 'alphabetic-asc treemacs-sorting))
  (should (equal '(treemacs--current-builtin-project-function
                   treemacs--current-directory-project-function)
                 treemacs--find-user-project-functions)))

(ert-deftest f6-load-enables-optional-modes ()
  "A plain load turns on follow, filewatch, fringe and git annotations."
  (skip-unless (and (executable-find "git") (plist-get f6-load-state :python)))
  (should (plist-get f6-load-state :follow))
  (should (plist-get f6-load-state :filewatch))
  (should (plist-get f6-load-state :fringe))
  (should (plist-get f6-load-state :git-mode))
  (should (eq 'deferred (plist-get f6-load-state :git-variant)))
  (should (equal 3 (plist-get f6-load-state :collapse-dirs))))

;;; Project root recognition

(ert-deftest f6-project-root-recognition ()
  (let* ((repo (directory-file-name
                (make-temp-file (expand-file-name "f6-root-" f6-reference-directory) t)))
         (plain (directory-file-name
                 (make-temp-file (expand-file-name "f6-plain-" f6-outside-directory) t)))
         (worktree (directory-file-name
                    (make-temp-file (expand-file-name "f6-worktree-" f6-reference-directory) t))))
    (unwind-protect
        (progn
          (make-directory (expand-file-name ".git" repo) t)
          (f6/mk repo "sub/nested.txt" "nested\n")
          (f6/mk plain "sub/deep.txt" "deep\n")
          ;; A `.git` file marks a worktree checkout.
          (f6/mk worktree ".git" (format "gitdir: %s\n" (expand-file-name ".git" repo)))
          (f6/mk worktree "sub/file.txt" "file\n")
          (cl-flet ((resolve-builtin (directory)
                      (with-temp-buffer
                        (setq default-directory (file-name-as-directory directory))
                        (treemacs--current-builtin-project-function)))
                    (resolve-cwd (directory)
                      (with-temp-buffer
                        (setq default-directory (file-name-as-directory directory))
                        (treemacs--current-directory-project-function))))
            ;; project.el resolves a nested directory to the repository root;
            ;; a directory without a VCS yields nil.
            (should (equal repo (resolve-builtin repo)))
            (should (equal repo (resolve-builtin (expand-file-name "sub" repo))))
            (should-not (resolve-builtin plain))
            (should-not (resolve-builtin (expand-file-name "sub" plain)))
            (should (equal worktree (resolve-builtin worktree)))
            ;; The fallback is the buffer's own directory, never a parent.
            (should (equal repo (resolve-cwd repo)))
            (should (equal (expand-file-name "sub" repo)
                           (resolve-cwd (expand-file-name "sub" repo))))
            (should (equal plain (resolve-cwd plain)))))
      (delete-directory repo t)
      (delete-directory plain t)
      (delete-directory worktree t))))

;;; Tree content

(ert-deftest f6-tree-default-text ()
  "Root line, directory-first ordering, dotfiles shown, .git hidden,
single-child directory chains collapsed."
  (f6/with-fixture
    (should (equal (format "%s\n  + Dir1\n  + b-dir\n  + chain/one/two\n  + dir2\n   .gitignore\n   .hidden.txt\n   1.txt\n   B-upper.txt\n   aaa.txt\n   ignored.log"
                           (treemacs--filename root))
                   (f6/tree-text)))))

(ert-deftest f6-tree-dotfiles-toggle ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-toggle-show-dotfiles))
    (should-not (string-match-p "gitignore" (f6/tree-text)))
    (should-not (string-match-p "hidden.txt" (f6/tree-text)))
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-toggle-show-dotfiles))
    (should (string-match-p "hidden.txt" (f6/tree-text)))))

(ert-deftest f6-tree-sorting-option ()
  "`s' asks for a sort method; the option itself reorders dirs and files."
  (f6/with-fixture
    (let ((treemacs-sorting 'alphabetic-desc))
      (with-current-buffer (treemacs-get-local-buffer) (treemacs-refresh)))
    (should (equal (format "%s\n  + dir2\n  + chain/one/two\n  + b-dir\n  + Dir1\n   ignored.log\n   aaa.txt\n   B-upper.txt\n   1.txt\n   .hidden.txt\n   .gitignore"
                           (treemacs--filename root))
                   (f6/tree-text)))))

(ert-deftest f6-tree-sort-command-asks ()
  "`s' selects a sort method interactively instead of cycling."
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (cl-letf (((symbol-function 'completing-read)
                 (lambda (&rest _) "Sort Alphabetically Descending")))
        (treemacs-resort))
      (should (eq 'alphabetic-desc treemacs-sorting))
      (should (string-match-p (regexp-quote "\n  + dir2") (f6/tree-text))))))

(ert-deftest f6-tree-tab-expands-and-collapses ()
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-TAB-action)
    (should (string-match-p "\n  - b-dir\n     inner.txt" (f6/tree-text)))
    (f6/goto-node "b-dir")
    (treemacs-TAB-action)
    (should-not (string-match-p "inner.txt" (f6/tree-text)))))

(ert-deftest f6-tree-expanded-dir-prefix-is-dash ()
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-TAB-action)
    (should (equal "  - b-dir" (f6/tree-line)))))

(ert-deftest f6-collapse-action-on-open-node-collapses ()
  "`h' collapses an open node like TAB."
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-TAB-action)
    (should (string-match-p "inner.txt" (f6/tree-text)))
    (f6/goto-node "b-dir")
    (treemacs-COLLAPSE-action)
    (should-not (string-match-p "inner.txt" (f6/tree-text)))
    (should (equal "  + b-dir" (f6/tree-line)))))

(ert-deftest f6-collapse-action-on-closed-node-goes-to-parent ()
  "`h' on a closed node moves to the parent instead of opening it."
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-COLLAPSE-action)
    (should (equal (treemacs--filename root) (f6/tree-line)))
    (should-not (string-match-p "inner.txt" (f6/tree-text)))))

(ert-deftest f6-collapse-parent-node ()
  "`H' moves to the parent node and closes it."
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-TAB-action)
    (f6/goto-node "inner.txt")
    (treemacs-collapse-parent-node nil)
    (should (equal "  + b-dir" (f6/tree-line)))
    (should-not (string-match-p "inner.txt" (f6/tree-text)))))

(ert-deftest f6-tree-tab-on-file-without-tags ()
  "TAB on a file node expands its (empty) tag section and changes nothing."
  (f6/with-fixture
    (f6/goto-node "aaa.txt")
    (let ((before (f6/tree-text)))
      (treemacs-TAB-action)
      (should (equal before (f6/tree-text))))))

(ert-deftest f6-tree-refresh-preserves-point-and-collapse ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-goto-file-node (expand-file-name "chain/one/two/deep.txt" root) proj)
      (should (string-match-p "deep.txt" (f6/tree-text)))
      (let ((line (f6/tree-line)))
        (treemacs-refresh)
        (should (equal line (f6/tree-line)))
        (should (string-match-p "deep.txt" (f6/tree-text)))))))

;;; Width

(ert-deftest f6-width-commands ()
  "`treemacs-width' is a body width: total = body + 1 in a batch frame."
  (f6/with-fixture
    (should (equal 35 (window-total-width (f6/tree-window))))
    (should (equal 34 (window-width (f6/tree-window))))
    (treemacs-increase-width)
    (should (equal 35 (window-width (f6/tree-window))))
    (treemacs-decrease-width)
    (should (equal 34 (window-width (f6/tree-window))))
    ;; `=' fits the window to the longest visible line and stores that value in
    ;; the option.
    (treemacs-fit-window-width)
    (should (< treemacs-width 35))
    (should (equal treemacs-width (window-width (f6/tree-window))))
    ;; A prefix argument re-applies the option value.
    (setq treemacs-width 35)
    (treemacs-set-width '(4))
    (should (equal 35 (window-width (f6/tree-window))))
    (should (equal 36 (window-total-width (f6/tree-window))))))

;;; Window lifecycle

(ert-deftest f6-side-window-parameters ()
  (f6/with-fixture
    (let ((window (f6/tree-window)))
      (should (eq 'left (window-parameter window 'window-side)))
      (should (eq t (window-parameter window 'no-delete-other-windows)))
      (should (equal 35 (window-total-width window)))
      (with-current-buffer (treemacs-get-local-buffer)
        (should (eq 'width window-size-fixed)))
      ;; The window is created with slot -1; redisplay runs
      ;; `window-configuration-change-hook', which does not happen in batch, so
      ;; call the hook directly to observe the normalized layout.
      (should (memql (window-parameter window 'window-slot) '(-1 0)))
      (with-selected-window (f6/tree-window)
        (treemacs--on-window-config-change))
      (should (equal 0 (window-parameter (f6/tree-window) 'window-slot)))
      (should (eq 'left (window-parameter (f6/tree-window) 'window-side)))
      (should (eq t (window-parameter (f6/tree-window) 'no-delete-other-windows))))))

(ert-deftest f6-init-selects-tree-window ()
  (f6/with-fixture
    (should (eq (selected-window) (f6/tree-window)))))

(ert-deftest f6-delete-other-windows-keeps-tree ()
  (f6/with-fixture
    (select-window (get-buffer-window current))
    (split-window-vertically)
    (should (equal 3 (length (window-list nil 'no-minibuf))))
    (delete-other-windows)
    (should (equal 2 (length (window-list nil 'no-minibuf))))
    (should (window-live-p (f6/tree-window)))
    (should (eq 'left (window-parameter (f6/tree-window) 'window-side)))))

(ert-deftest f6-delete-other-windows-from-tree-errors ()
  "A side window can never become the only window."
  (f6/with-fixture
    (should-error (delete-other-windows) :type 'error)
    (should (window-live-p (f6/tree-window)))))

(ert-deftest f6-delete-window-closes-tree ()
  "The side window parameter only protects against `delete-other-windows'."
  (f6/with-fixture
    (select-window (f6/tree-window))
    (delete-window)
    (should-not (f6/tree-window))
    (should (eq 'exists (treemacs-current-visibility)))
    (should (treemacs-get-local-buffer))))

(ert-deftest f6-other-window-reaches-tree ()
  (f6/with-fixture
    (select-window (get-buffer-window current))
    (other-window 1)
    (should (eq (selected-window) (f6/tree-window)))
    (other-window 1)
    (should (equal (buffer-name (window-buffer (selected-window))) "aaa.txt"))))

(ert-deftest f6-toggle-three-states ()
  (f6/with-fixture
    ;; visible -> hidden
    (treemacs)
    (should-not (f6/tree-window))
    (should (eq 'exists (treemacs-current-visibility)))
    ;; exists -> shown and selected
    (treemacs)
    (should (eq 'visible (treemacs-current-visibility)))
    (should (eq (selected-window) (f6/tree-window)))
    ;; hidden state disappears when the buffer is killed
    (with-current-buffer (treemacs-get-local-buffer)
      (call-interactively #'treemacs-kill-buffer))
    (should (eq 'none (treemacs-current-visibility)))))

(ert-deftest f6-quit-buries-buffer ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer) (treemacs-quit))
    (should-not (f6/tree-window))
    (should (eq 'exists (treemacs-current-visibility)))
    (should (buffer-live-p (treemacs-get-local-buffer)))))

(ert-deftest f6-kill-buffer-removes-everything ()
  (f6/with-fixture
    (let ((buffer (treemacs-get-local-buffer)))
      (with-current-buffer buffer
        (call-interactively #'treemacs-kill-buffer))
      (should-not (buffer-live-p buffer))
      (should-not (f6/tree-window))
      (should (eq 'none (treemacs-current-visibility))))))

;;; Visiting files

(ert-deftest f6-ret-visits-file-beside-tree ()
  "RET opens in the window next to the tree, selects it and keeps the tree."
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-goto-file-node (expand-file-name "dir2/x.txt" root) proj)
      (treemacs-RET-action))
    (should (equal "x.txt" (buffer-name (window-buffer (selected-window)))))
    (should (window-live-p (f6/tree-window)))
    (should (equal 2 (length (window-list nil 'no-minibuf))))
    (should (eq 'visible (treemacs-current-visibility)))))

(ert-deftest f6-ret-single-prefix-stays-in-tree ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-goto-file-node (expand-file-name "dir2/x.txt" root) proj)
      (treemacs-RET-action '(4)))
    (should (eq (selected-window) (f6/tree-window)))
    (should (get-file-buffer (expand-file-name "dir2/x.txt" root)))
    ;; The file reuses the window next to the tree, no extra split.
    (should (equal 2 (length (window-list nil 'no-minibuf))))))

(ert-deftest f6-ret-double-prefix-closes-tree ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (treemacs-goto-file-node (expand-file-name "dir2/x.txt" root) proj)
      (treemacs-RET-action '(16)))
    (should-not (f6/tree-window))
    (should (eq 'exists (treemacs-current-visibility)))
    (should (equal "x.txt" (buffer-name (window-buffer (selected-window)))))))

(ert-deftest f6-ret-visits-directory-with-dired ()
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (let ((treemacs-RET-actions-config
           (cons (cons 'dir-node-closed #'treemacs-visit-node-no-split)
                 treemacs-RET-actions-config)))
      (treemacs-RET-action))
    (should (eq major-mode 'dired-mode))))

(ert-deftest f6-ret-on-directory-toggles ()
  (f6/with-fixture
    (f6/goto-node "b-dir")
    (treemacs-RET-action)
    (should (string-match-p "inner.txt" (f6/tree-text)))
    (f6/goto-node "b-dir")
    (treemacs-RET-action)
    (should-not (string-match-p "inner.txt" (f6/tree-text)))))

;;; Find file / select window

(ert-deftest f6-find-file-moves-tree-point-only ()
  "With the tree visible, find-file locates the node but keeps the file window."
  (f6/with-fixture
    (select-window (get-buffer-window current))
    (treemacs-find-file)
    (should (equal "   aaa.txt" (f6/tree-line)))
    (should (equal "aaa.txt" (buffer-name (window-buffer (selected-window)))))))

(ert-deftest f6-find-file-reveals-hidden-tree ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer) (treemacs-quit))
    (switch-to-buffer (find-file-noselect (expand-file-name "dir2/x.txt" root)))
    (treemacs-find-file)
    (should (eq 'visible (treemacs-current-visibility)))
    (should (equal "     x.txt" (f6/tree-line)))
    ;; Revealing the tree does not steal the selection from the file window.
    (should (equal "x.txt" (buffer-name (window-buffer (selected-window)))))))

(ert-deftest f6-select-window-move-back ()
  (f6/with-fixture
    (select-window (get-buffer-window current))
    (treemacs-select-window)
    (should (eq (selected-window) (f6/tree-window)))
    (treemacs-select-window)
    (should (equal "aaa.txt" (buffer-name (window-buffer (selected-window)))))))

;;; Key bindings reachable from the tree buffer

(ert-deftest f6-key-bindings ()
  (f6/with-fixture
    (with-current-buffer (treemacs-get-local-buffer)
      (cl-flet ((bound (key) (key-binding (kbd key) t)))
        (dolist (pair '(("TAB" . treemacs-TAB-action)
                        ("RET" . treemacs-RET-action)
                        ("l" . treemacs-RET-action)
                        ("h" . treemacs-COLLAPSE-action)
                        ("n" . treemacs-next-line)
                        ("p" . treemacs-previous-line)
                        ("M-n" . treemacs-next-neighbour)
                        ("M-p" . treemacs-previous-neighbour)
                        ("C-n" . next-line)
                        ("C-p" . previous-line)
                        ("u" . treemacs-goto-parent-node)
                        ("H" . treemacs-collapse-parent-node)
                        ("q" . treemacs-quit)
                        ("Q" . treemacs-kill-buffer)
                        ("g" . treemacs-refresh)
                        ("r" . treemacs-refresh)
                        ("s" . treemacs-resort)
                        ("P" . treemacs-peek-mode)
                        ("w" . treemacs-set-width)
                        ("<" . treemacs-decrease-width)
                        (">" . treemacs-increase-width)
                        ("=" . treemacs-fit-window-width)
                        ("W" . treemacs-extra-wide-toggle)
                        ("C-j" . treemacs-next-project)
                        ("C-k" . treemacs-previous-project)
                        ("t h" . treemacs-toggle-show-dotfiles)
                        ("t i" . treemacs-hide-gitignored-files-mode)
                        ("t f" . treemacs-follow-mode)
                        ("t w" . treemacs-toggle-fixed-width)
                        ("o o" . treemacs-visit-node-no-split)
                        ("o v" . treemacs-visit-node-vertical-split)
                        ("o h" . treemacs-visit-node-horizontal-split)
                        ("o r" . treemacs-visit-node-in-most-recently-used-window)
                        ("o c" . treemacs-visit-node-close-treemacs)
                        ("o x" . treemacs-visit-node-in-external-application)))
          (should (eq (cdr pair) (bound (car pair)))))))))
