cli-about = 一个用于上传本地文件到 S3 兼容对象存储的小型 CLI。
cmd-target-about = 管理上传 target
cmd-upload-about = 上传文件或目录
cmd-log-about = 管理诊断日志
cmd-language-about = 管理 CLI 语言
cmd-target-add-about = 添加或更新上传 target
cmd-target-list-about = 列出已配置的上传 target
cmd-target-use-about = 设置默认上传 target
cmd-target-remove-about = 移除上传 target
cmd-log-export-about = 导出解密后的诊断日志
cmd-log-clear-about = 清空加密诊断日志
cmd-language-show-about = 显示当前 CLI 语言
cmd-language-use-about = 设置 CLI 语言

prompt-bucket = 存储桶
prompt-endpoint = 端点
prompt-region = 区域
prompt-public-base-url = 公开访问基础 URL
prompt-access-key-id = Access key ID
prompt-secret-access-key = Secret access key
prompt-save-access-keys-now = 现在保存访问密钥吗？[Y/n]: 
prompt-please-answer-yes-no = 请输入 y 或 n。
prompt-cannot-be-empty = { $label } 不能为空。

target-added = 已添加 target `{ $name }`。
target-using = 正在使用 target `{ $name }`。
target-removed = 已移除 target `{ $name }`。
target-no-targets-configured = 还没有配置 target。
target-checking-connectivity = 正在检查 target 连通性...
target-connectivity-passed = Target 连通性检查通过。
target-connectivity-skipped-no-credentials = 未保存访问密钥，已跳过 target 连通性检查。

language-current = 当前语言：{ $language }

log-exported = 已导出诊断日志到 { $path }（{ $count } 条事件）。分享前请先检查内容。
log-cleared = 已清空诊断日志。

upload-missing-credentials = target `{ $target }` 缺少访问凭证；请运行 `filelift target add { $target }` 并选择保存访问密钥。
