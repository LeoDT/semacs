;;; export-treemacs-keys.el --- dump treemacs-mode keymap -*- lexical-binding: t; -*-
;;
;; Fixed environment: GNU Emacs 31.1, treemacs 20251226.1307 (commit
;; 2ab5a3c89fa01bbbd99de9b8986908b2bc5a7b49), load-path as in
;; tests/emacs/README.md.
;;
;; Usage, from the repository root:
;;   emacs -Q --batch -L ... -l tests/emacs/export-treemacs-keys.el > tests/emacs/f6-bindings.tsv
;;
;; Evidence boundary: a raw keymap dump. Escape-prefixed (M-) and
;; terminal-only translations are resolved by `key-binding' inside a real
;; treemacs buffer instead, see f6-treemacs.el.

(require 'treemacs)

(defvar reference/bindings nil)

(defun reference/walk (map prefix)
  (map-keymap
   (lambda (event definition)
     (let ((key (vconcat prefix (vector event))))
       (cond ((keymapp definition) (reference/walk definition key))
             ((symbolp definition)
              (push (format "%s\t%s" (key-description key) definition)
                    reference/bindings))
             (t (push (format "%s\tlambda" (key-description key))
                      reference/bindings)))))
   map))

(princ (format "# emacs-version: %s\n" emacs-version))
(princ (format "# treemacs-version: %s\n" treemacs-version))
(princ "# driving: emacs -Q --batch, treemacs-mode-map walk (not a command loop)\n")
(princ "#\n")
(reference/walk treemacs-mode-map [])
(dolist (line (sort reference/bindings #'string<))
  (princ (concat line "\n")))
