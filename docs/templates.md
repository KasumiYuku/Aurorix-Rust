# 模板
模板把数据填进 Markdown 文本，按插件命名空间存放，走 `assets/<id>/templates/` 与 `templates/override/` 覆盖目录。
覆盖目录按秒轮询，改完模板文件，下一次渲染就生效，不用重启。
模板 id 取文件名，命名空间取插件 id，`_global/` 目录下的覆盖文件进全局命名空间。

## 变量
`{{名字}}` 从参数取值；有占位符取不到值时整次渲染失败，不输出半成品。
```rust
use aurorix_template::{args_map, Registry, TemplateKind};

//assets/rating/templates/b30.md 里写:
//### {{title}}
//平均 {{average}}
let args = args_map! { "title" => "Best 30", "average" => 13.30f64 };
let text = templates.fill(TemplateKind::Markdown, "rating", "b30", &args)
    .unwrap_or_default();
```
`templates` 是插件持有的 `Registry`，第二个参数是命名空间（插件 id）；列表用 `{{#each key}}` 遍历。
嵌套 map 展开成 `父.子`，列表展开成 `父.#0`、`父.#1`；插件命名空间取不到时回退全局模板。

## 覆盖目录
把同名文件放进 `templates/override/<插件 id>/` 就覆盖插件自带的模板，放进 `templates/override/_global/` 则对所有插件生效。
覆盖目录按秒轮询，改完下一次渲染就生效，不用重启；删掉覆盖文件即回退到插件版本。

## 内联按钮
`{{inline "显示文字" "/指令"}}` 渲染成平台的内联按钮，点一下就把指令发出去。
```md
{{inline "再查一次" "/b30 --top=10"}}      → <qqbot-cmd-input text="%2Fb30%20--top%3D10" show="…" />
{{inline "引用这条" "/p me" "true"}}       → 同上，末尾多一个 reference="true"
[按钮](mqqapi://aio/misc?jumpUrl=https%3A%2F%2Fexample.com)
```
`text` 是指令原文、`show` 是显示文字，都要百分号编码；第三个操作数只有 `true` / `1` / `yes` 为真。
也可以直接用 `mqqapi://` 链接写按钮，地址里的参数同样百分号编码。

## 图片尺寸
尺寸标记写在 alt 里，形如 `#宽px #高px`。
```md
渲染前:
![封面 #320px #180px](https://example.com/cover.png)
![头像 #64px #64px](https://example.com/avatar.png)

超出平台上限会被收窄:
![封面 #350px #438px](https://example.com/cover.png)
```
平台对单图有上限，超出的尺寸标记会被改成合法值再发送。
alt 里的说明文字会保留，只替换尺寸标记。

## 页脚按钮
`config.json` 里配好，框架在发送前自动追加到键盘末尾。
```json
"footer_buttons_enabled": true,
"footer_buttons": [
  "点赞 | counter | 感谢 {{count}} 次",
  "帮助 | command | /help"
],
"footer_buttons_group": ["再查一次 | repeat"]
```
一行一个按钮，格式是 `标签 | 类型 | 参数`；`footer_buttons_group` 等场景键覆盖各自场景。
插件用 `Outgoing::no_footer()` 或 `CommandSpec::no_footer()` 关掉单条回复的页脚，原有键盘照常保留。
`footer_buttons_c2c` / `_channel` / `_dm` 覆盖各自场景，取不到时回退 `footer_buttons`。
开关 `footer_buttons_enabled` 设为 false 时整体关闭。
