;;; baseline.el --- dump state after every command in a real Emacs command loop -*- lexical-binding: t; -*-
;;
;; Driven by drive.py through a pty with `emacs -Q -nw`, so undo grouping comes
;; from the real command loop instead of calling functions from batch.
;; Environment fixed by docs/emacs-fidelity.md section 5.

(defvar spike/state-file (getenv "SPIKE_STATE")
  "File that receives one line per command.")

(defvar spike/initial-text (or (getenv "SPIKE_INITIAL") ""))

(defun spike/dump (tag)
  (let ((line (format "{\"tag\":%s,\"text\":%s,\"point\":%d,\"mark\":%s,\"active\":%s,\"modified\":%s,\"msg\":%s}\n"
                      (if tag (format "\"%s\"" tag) "null")
                      (concat "\"" (base64-encode-string (encode-coding-string (buffer-string) 'utf-8) t) "\"")
                      (point)
                      (if (mark t) (number-to-string (mark t)) "null")
                      (if mark-active "true" "false")
                      (if (buffer-modified-p) "true" "false")
                      (prin1-to-string (substring-no-properties (or (current-message) ""))))))
    (let ((inhibit-message t))
      (with-temp-buffer
        (insert line)
        (write-region (point-min) (point-max) spike/state-file t 'silent)))))

(defun spike/setup ()
  (switch-to-buffer (get-buffer-create "*spike*"))
  (fundamental-mode)
  (transient-mark-mode 1)
  (setq buffer-undo-list nil)
  (erase-buffer)
  (insert spike/initial-text)
  (goto-char (point-max))
  (setq mark-active nil)
  (setq buffer-undo-list nil)
  (set-buffer-modified-p nil)
  (setq buffer-undo-list nil)
  (local-set-key (kbd "C-c m") (lambda () (interactive) (set-buffer-modified-p nil) (message "saved")))
  (local-set-key (kbd "C-c r") (lambda () (interactive) (setq buffer-read-only t) (message "readonly")))
  (add-hook 'post-command-hook (lambda () (spike/dump nil))))

(add-hook 'window-setup-hook #'spike/setup)
