cli-about = A small CLI for lifting local files to S3-compatible object storage.
cmd-target-about = Manage upload targets
cmd-upload-about = Upload a file or directory
cmd-log-about = Manage diagnostic logs
cmd-language-about = Manage CLI language
cmd-completions-about = Generate shell completions
cmd-target-add-about = Add an upload target
cmd-target-update-about = Update an upload target
cmd-target-list-about = List configured upload targets
cmd-target-use-about = Set the default upload target
cmd-target-remove-about = Remove an upload target
cmd-log-export-about = Export decrypted diagnostic logs
cmd-log-clear-about = Clear encrypted diagnostic logs
cmd-language-show-about = Show the current CLI language
cmd-language-use-about = Set the CLI language

prompt-bucket = Bucket
prompt-provider = Provider
prompt-endpoint = Endpoint
prompt-region = Region
prompt-public-base-url = Public base URL
prompt-access-key-id = Access key ID
prompt-secret-access-key = Secret access key
prompt-save-access-keys-now = Save access keys now? [Y/n]: 
prompt-please-answer-yes-no = Please answer y or n.
prompt-cannot-be-empty = { $label } cannot be empty.

target-added = Added target `{ $name }`.
target-updated = Updated target `{ $name }`.
target-using = Using target `{ $name }`.
target-removed = Removed target `{ $name }`.
target-draft-resuming = Resuming draft target `{ $name }`.
target-draft-saved = Saved draft target `{ $name }`; rerun `filelift target add { $name }` to resume.
target-no-targets-configured = No targets configured.
target-checking-connectivity = Checking target connectivity...
target-connectivity-passed = Target connectivity check passed.
target-connectivity-skipped-no-credentials = Skipped target connectivity check because no access keys were saved.

language-current = Current language: { $language }

log-exported = Exported diagnostic log to { $path } ({ $count } events). Review it before sharing.
log-cleared = Cleared diagnostic logs.

upload-missing-credentials = Missing credentials for target `{ $target }`; run `filelift target update { $target }` to save access keys.
