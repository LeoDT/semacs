;;; m1-editing.el --- command routing reference semantics -*- lexical-binding: t; -*-
;;
;; Direct function calls, not command-loop or GUI evidence.
;; Run: emacs -Q --batch -l tests/emacs/m1-editing.el -f ert-run-tests-batch-and-exit

(require 'ert)

(ert-deftest reference/m1-character-motion ()
  (should (equal emacs-version "31.1"))
  (with-temp-buffer
    (fundamental-mode)
    (insert "中é🙂\nxyz")
    (goto-char (point-min))
    (dolist (expected '(3 4 6 10))
      (forward-char)
      (should (= (string-bytes (buffer-substring (point-min) (point))) expected)))
    (move-beginning-of-line 1)
    (should (= (point) 1))
    (move-end-of-line 1)
    (should (= (point) 5))))

(ert-deftest reference/m1-forward-delete-region ()
  (should (equal emacs-version "31.1"))
  (dolist (command '(delete-forward-char delete-char))
    (with-temp-buffer
      (fundamental-mode)
      (transient-mark-mode 1)
      (insert "abcdef")
      (goto-char 2)
      (set-mark (point))
      (goto-char 5)
      (setq mark-active t)
      (funcall command 1)
      (should (equal (buffer-string)
                     (if (eq command 'delete-forward-char) "aef" "abcdf"))))))

(ert-deftest reference/m1-escape-quit ()
  (should (equal emacs-version "31.1"))
  (with-temp-buffer
    (fundamental-mode)
    (transient-mark-mode 1)
    (insert "abc")
    (goto-char 1)
    (set-mark (point))
    (setq mark-active t)
    (goto-char 3)
    (keyboard-escape-quit)
    (should (= (point) 3))
    (should (= (mark) 1))
    (should-not mark-active)
    (should-not (current-message))))
