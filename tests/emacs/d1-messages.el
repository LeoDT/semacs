;;; d1-messages.el --- isolated message reference -*- lexical-binding: t; -*-

(require 'ert)
(require 'help)
(require 'minibuffer)

(defconst d1/defaults (list message-log-max max-mini-window-height resize-mini-windows
                           minibuffer-message-timeout minibuffer-message-clear-timeout))

(ert-deftest d1-message-defaults ()
  (should (equal emacs-version "31.1"))
  (should (eq (key-binding (kbd "C-h e")) 'view-echo-area-messages))
  (should (equal d1/defaults '(1000 0.25 grow-only 2 nil))))

(ert-deftest d1-log-preserves-lines-and-folds-adjacent-repeats ()
  (let ((message-log-max 1000))
    (with-current-buffer (messages-buffer)
      (let ((inhibit-read-only t)) (erase-buffer)))
    (message "same")
    (message "same")
    (message "one\ntwo")
    (message "one\ntwo")
    (message "other")
    (with-current-buffer (messages-buffer)
      (should (equal (buffer-string) "same [2 times]\none\ntwo\none\ntwo\nother\n"))
      (should buffer-read-only))
    (message "%s" (make-string 10000 ?x))
    (with-current-buffer (messages-buffer)
      (should (string-suffix-p (concat (make-string 10000 ?x) "\n") (buffer-string))))))

(ert-deftest d1-view-messages-displays-without-selecting ()
  (save-window-excursion
    (delete-other-windows)
    (let ((origin (selected-window)))
      (view-echo-area-messages)
      (should (eq (selected-window) origin))
      (let ((window (get-buffer-window (messages-buffer))))
        (should (window-live-p window))
        (should (= (window-point window)
                   (with-current-buffer (messages-buffer) (point-max))))))))
