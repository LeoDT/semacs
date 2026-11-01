;;; m1-indent.el --- M1 fundamental-mode indentation reference -*- lexical-binding: t; -*-
;;
;; Direct interactive function calls, not command-loop or GUI evidence.
;; Run: emacs -Q --batch -l tests/emacs/m1-indent.el -f ert-run-tests-batch-and-exit

(require 'ert)

(ert-deftest reference/m1-indent-options ()
  (should (equal emacs-version "31.1"))
  (with-temp-buffer
    (fundamental-mode)
    (dolist (option '((tab-width . 8)
                      (indent-tabs-mode . t)
                      (tab-always-indent . t)
                      (standard-indent . 4)
                      (indent-line-function . indent-relative)
                      (electric-indent-mode . t)
                      (electric-indent-inhibit . nil)
                      (electric-indent-chars . (10))))
      (ert-info ((format "option: %s" (car option)))
        (should (equal (symbol-value (car option)) (cdr option)))))))

(ert-deftest reference/m1-indent-results ()
  (should (equal emacs-version "31.1"))
  (dolist (case '(("" 1 newline "\n" 2)
                  ("" 1 electric-newline-and-maybe-indent "\n" 2)
                  ("" 1 indent-for-tab-command "\t" 2)
                  ("    abc" 8 newline "    abc\n    " 13)
                  ("    abc" 8 electric-newline-and-maybe-indent "    abc\n" 9)
                  ("    abc" 8 indent-for-tab-command "    abc\t" 9)
                  ("    abc" 3 newline "\nabc" 2)
                  ("    abc" 3 electric-newline-and-maybe-indent "  \n  abc" 4)
                  ("    abc" 3 indent-for-tab-command "\t  abc" 2)
                  ("    abc\n" 9 newline "    abc\n\n" 10)
                  ("abc   def" 7 newline "abc\ndef" 5)
                  ("        abc" 12 newline "        abc\n\t" 14)
                  ("\tfoo" 3 newline "\tf\n\too" 5)
                  ("    abc\n" 9 electric-newline-and-maybe-indent "    abc\n\n" 10)
                  ("    abc\n" 9 indent-for-tab-command "    abc\n    " 13)
                  ("  foo bar\nxx" 13 indent-for-tab-command "  foo bar\nxx    " 17)
                  ("\tfoo\n" 6 indent-for-tab-command "\tfoo\n\t" 7)
                  ("    \n" 6 indent-for-tab-command "    \n    " 10)
                  ("foo bar\n" 9 indent-for-tab-command "foo bar\n    " 13)
                  ("foo bar\n  " 11 indent-for-tab-command "foo bar\n    " 13)
                  ("foo bar\n      " 15 indent-for-tab-command "foo bar\n\t" 10)
                  ("  foo bar\n\n" 12 indent-for-tab-command "  foo bar\n\n  " 14)
                  ("中 文\n" 5 indent-for-tab-command "中 文\n   " 8)
                  ("abc  " 6 indent-for-tab-command "abc\t" 5)
                  ("abc\ndef" 6 indent-for-tab-command "abc\nd\tef" 7)
                  ("foo\tbar\n" 9 indent-for-tab-command "foo\tbar\n\t" 10)))
    (ert-info ((format "case: %S" case))
      (with-temp-buffer
        (fundamental-mode)
        (setq-local tab-width 8
                    indent-tabs-mode t
                    tab-always-indent t
                    standard-indent 4
                    indent-line-function #'indent-relative
                    electric-indent-inhibit nil
                    electric-indent-chars '(10))
        (let ((electric-indent-mode t))
          (insert (nth 0 case))
          (goto-char (nth 1 case))
          (call-interactively (nth 2 case))
          (should (equal (buffer-string) (nth 3 case)))
          (should (= (point) (nth 4 case))))))))
