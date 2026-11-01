;;; export-keys.el --- export v1 default key bindings from reference Emacs -*- lexical-binding: t; -*-
;;
;; Reference environment fixed by docs/emacs-fidelity.md section 5:
;;   GNU Emacs 31.1, emacs -Q, fundamental-mode, transient-mark-mode on.
;; Usage: emacs -Q --batch -l tests/emacs/export-keys.el
;;
;; Evidence boundary: this drives key-binding lookups, not a real command loop.

(setq inhibit-startup-screen t)

(defvar reference/keys
  '("C-f" "C-b" "C-p" "C-n" "C-a" "C-e"
    "M-f" "M-b" "M-<" "M->"
    "C-v" "M-v"
    "C-d" "DEL" "backspace" "C-k"
    "C-w" "M-w" "C-y" "M-y"
    "C-@" "C-SPC" "C-x C-x" "C-g"
    "C-/" "C-_" "C-x u" "C-?" "C-M-_"
    "M-x"
    "C-x C-f" "C-x C-s" "C-x C-w" "C-x b" "C-x k" "C-x C-c"
    "C-s" "C-r" "C-u"
    "C-x 1" "C-x 2" "C-x 3" "C-x o" "C-x a"
    "RET" "<return>" "TAB" "<tab>" "C-i" "C-j" "C-m" "C-h"
    "<backspace>" "<delete>" "<deletechar>" "C-l"
    "ESC" "M-ESC" "ESC ESC" "ESC ESC ESC" "ESC f" "ESC C-_"
    "ESC C-g" "C-x C-g"))

(defun reference/describe-binding (key)
  (let ((cmd (condition-case err
                 (key-binding (kbd key) t)
               (error (format "ERROR:%S" err)))))
    (cond ((null cmd) "nil")
          ((keymapp cmd) "PREFIX")
          ((stringp cmd) cmd)
          (t (symbol-name cmd)))))

(defun reference/export ()
  (with-temp-buffer
    (fundamental-mode)
    (transient-mark-mode 1)
    (princ (format "# emacs-version: %s\n" emacs-version))
    (princ (format "# major-mode: %s\n" major-mode))
    (princ (format "# transient-mark-mode: %s\n" transient-mark-mode))
    (princ (format "# undo-limit: %s\n" undo-limit))
    (princ (format "# undo-strong-limit: %s\n" undo-strong-limit))
    (princ (format "# undo-outer-limit: %s\n" undo-outer-limit))
    (dolist (option '(tab-width indent-tabs-mode tab-always-indent standard-indent
                      indent-line-function electric-indent-mode electric-indent-inhibit
                      electric-indent-chars))
      (princ (format "# %s: %S\n" option (symbol-value option))))
    (princ "# driving: emacs -Q --batch, key-binding lookup (not a command loop)\n")
    (princ "#\n")
    (dolist (key reference/keys)
      (princ (format "%s\t%s\n" key (reference/describe-binding key))))))

(reference/export)
