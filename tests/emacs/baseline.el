;;; baseline.el --- dump state after every command in a real Emacs command loop -*- lexical-binding: t; -*-
;;
;; Driven by drive.py through a pty with `emacs -Q -nw`, so undo grouping comes
;; from the real command loop instead of calling functions from batch.
;; Environment fixed by docs/emacs-fidelity.md section 5.

(defvar reference/state-file (getenv "EMACS_REFERENCE_STATE")
  "File that receives one line per command.")

(defvar reference/initial-text (or (getenv "EMACS_REFERENCE_INITIAL") ""))

(defun reference/dump (tag)
  (let ((line (format "{\"tag\":%s,\"kill_ring\":%s,\"text\":%s,\"point\":%d,\"mark\":%s,\"active\":%s,\"modified\":%s,\"prompt_end\":%s,\"msg\":%s}\n"
                      (if tag (format "\"%s\"" tag) "null")
                      (concat "[" (mapconcat
                                   (lambda (text)
                                     (prin1-to-string
                                      (base64-encode-string (encode-coding-string text 'utf-8) t)))
                                   kill-ring ",") "]")
                      (concat "\"" (base64-encode-string (encode-coding-string (buffer-string) 'utf-8) t) "\"")
                      (point)
                      (if (mark t) (number-to-string (mark t)) "null")
                      (if mark-active "true" "false")
                      (if (buffer-modified-p) "true" "false")
                      (if (minibufferp) (number-to-string (minibuffer-prompt-end)) "null")
                      (prin1-to-string (substring-no-properties (or (current-message) ""))))))
    (let ((inhibit-message t))
      (with-temp-buffer
        (insert line)
        (write-region (point-min) (point-max) reference/state-file t 'silent)))))

(defun reference/setup ()
  (switch-to-buffer (get-buffer-create "*reference*"))
  (fundamental-mode)
  (transient-mark-mode 1)
  (setq kill-ring nil
        kill-ring-yank-pointer nil
        interprogram-cut-function nil
        interprogram-paste-function nil
        copy-region-blink-delay 0)
  (setq buffer-undo-list nil)
  (erase-buffer)
  (insert reference/initial-text)
  (goto-char (point-max))
  (setq mark-active nil)
  (setq buffer-undo-list nil)
  (set-buffer-modified-p nil)
  (setq buffer-undo-list nil)
  (local-set-key (kbd "C-c m") (lambda () (interactive) (set-buffer-modified-p nil) (message "saved")))
  (local-set-key (kbd "C-c r") (lambda () (interactive) (setq buffer-read-only t) (message "readonly")))
  (local-set-key (kbd "C-c t")
                 (lambda () (interactive) (read-from-minibuffer "Input: ")))
  (add-hook 'post-command-hook (lambda () (reference/dump nil))))

(add-hook 'window-setup-hook #'reference/setup)
