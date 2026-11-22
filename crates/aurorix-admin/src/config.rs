use aurorix_core::constant;
use aurorix_db::AppConfig;
use serde_json::{json, Map, Value};

pub const WRITABLE_KEYS: &[&str] = &[
    "bot_maintenance",
    "bot_maintenance_message",
    "port",
    "listen_addr",
    "appid",
    "secret",
    "proxy",
    "gateway_url",
    "database",
    "data_dir",
    "protocol",
    "intents",
    "prefixes",
    "admin_password",
    "log_level",
    "global_markdown",
    "image_limits_enabled",
    "image_max_width",
    "image_max_height",
    "retry_when",
    "upload_threshold",
    "keyboard_font_size",
    "footer_buttons_enabled",
    "footer_buttons",
    "footer_buttons_group",
    "footer_buttons_c2c",
    "footer_buttons_channel",
    "footer_buttons_dm",
    "welcome_enabled",
    "welcome_messages",
    "farewell_enabled",
    "farewell_messages",
    "welcome_extra",
    "welcome_buttons",
    "farewell_extra",
    "farewell_buttons",
];

pub const RESTART_KEYS: &[&str] = &[
    "port",
    "listen_addr",
    "appid",
    "secret",
    "proxy",
    "gateway_url",
    "database",
    "data_dir",
    "protocol",
    "intents",
    "prefixes",
];

pub const SECRET_MASK: &str = "********";

pub const SECRET_KEYS: &[&str] = &["admin_password", "secret"];

const NO_PREFIX: &str = "无前缀";

const PREFIX_OPTIONS: &[&str] = &["!", "/", "#", NO_PREFIX];

const INTENT_OPTIONS: &[&str] = &[
    "GROUP_AT_MESSAGE_CREATE",
    "GROUP_MESSAGE_CREATE",
    "C2C_MESSAGE_CREATE",
    "INTERACTION_CREATE",
    "GROUP_JOIN_REQUEST",
    "GROUP_MEMBER_ADD",
    "GROUP_MEMBER_REMOVE",
    "GROUP_ADD_ROBOT",
    "GROUP_DEL_ROBOT",
    "GROUP_MSG_RECEIVE",
    "GROUP_MSG_REJECT",
    "C2C_MSG_RECEIVE",
    "C2C_MSG_REJECT",
    "FRIEND_ADD",
    "FRIEND_DEL",
    "MESSAGE_AUDIT_PASS",
    "MESSAGE_AUDIT_REJECT",
    "AT_MESSAGE_CREATE",
    "MESSAGE_CREATE",
    "DIRECT_MESSAGE_CREATE",
    "GUILD_CREATE",
    "GUILD_UPDATE",
    "GUILD_DELETE",
    "CHANNEL_CREATE",
    "CHANNEL_UPDATE",
    "CHANNEL_DELETE",
    "GUILD_MEMBER_ADD",
    "GUILD_MEMBER_REMOVE",
];

#[derive(Debug, Clone, Copy)]
pub struct ConfigGroup {
    pub id: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
}

#[derive(Debug, Clone, Copy)]
pub struct CoreField {
    pub key: &'static str,
    pub label: &'static str,
    pub desc: &'static str,
    pub kind: &'static str,
    pub options: &'static [&'static str],
    pub hot: bool,
    pub restart: bool,
    pub dedicated: bool,
    pub placeholder: &'static str,
    pub group: &'static str,
    pub unit: &'static str,
}

fn base(
    group: &'static str,
    key: &'static str,
    label: &'static str,
    kind: &'static str,
) -> CoreField {
    CoreField {
        key,
        label,
        desc: "",
        kind,
        options: &[],
        hot: false,
        restart: false,
        dedicated: false,
        placeholder: "",
        group,
        unit: "",
    }
}

pub fn config_groups() -> &'static [ConfigGroup] {
    &[
        ConfigGroup {
            id: "maintenance",
            label: "维护开关",
            desc: "先把 Bot 停下来, 再慢慢排查; 只有这个页面能关掉它。",
        },
        ConfigGroup {
            id: "connection",
            label: "连接与协议",
            desc: "改了要重启进程才生效。",
        },
        ConfigGroup {
            id: "console",
            label: "管理台",
            desc: "本控制台的登录口令, 保存即生效, 旧会话立即失效。",
        },
        ConfigGroup {
            id: "logging",
            label: "日志",
            desc: "控制台输出的最低级别, 保存即生效, 不用重启。",
        },
        ConfigGroup {
            id: "message",
            label: "消息与图片",
            desc: "消息怎么发出去: Markdown、图片尺寸、重试错误码与分片上传。",
        },
        ConfigGroup {
            id: "footer",
            label: "底部按钮",
            desc: "每条消息末尾追加的按钮板, 群聊/单聊/频道/私信可以各写一套。",
        },
        ConfigGroup {
            id: "member",
            label: "入退群",
            desc: "有人进群、有人退群时发什么; 文案留空就不发。",
        },
    ]
}

pub fn core_specs() -> Vec<CoreField> {
    vec![
        CoreField {
            desc: "打开后所有人(含管理员与主人)都触发不了任何功能, 一律回下面这句话; 只能在本控制台关掉。",
            hot: true,
            dedicated: true,
            ..base("maintenance", "bot_maintenance", "Bot 全局维护", "bool")
        },
        CoreField {
            desc: "留空用框架默认话术",
            placeholder: constant::DEFAULT_MAINTENANCE_MESSAGE,
            hot: true,
            dedicated: true,
            ..base(
                "maintenance",
                "bot_maintenance_message",
                "全局维护提示语",
                "textarea",
            )
        },
        CoreField {
            restart: true,
            ..base("connection", "port", "监听端口", "number")
        },
        CoreField {
            desc: "管理台与 webhook 回调的绑定地址; 留空按协议取默认值(websocket 只绑 127.0.0.1, webhook 绑 0.0.0.0)。改完要重启",
            restart: true,
            ..base("connection", "listen_addr", "HTTP 监听地址", "text")
        },
        CoreField {
            restart: true,
            ..base("connection", "appid", "机器人 AppID", "text")
        },
        CoreField {
            desc: "不会回显, 留空保持原值",
            restart: true,
            ..base("connection", "secret", "AppSecret", "secret")
        },
        CoreField {
            restart: true,
            ..base("connection", "proxy", "API 代理地址", "text")
        },
        CoreField {
            desc: "填 wss:// 地址可接 webhook 转 websocket 中转站, 留空走官方网关; 仅 websocket 模式生效",
            restart: true,
            ..base("connection", "gateway_url", "自定义网关地址", "text")
        },
        CoreField {
            restart: true,
            ..base("connection", "database", "PostgreSQL 连接串", "text")
        },
        CoreField {
            desc: "本地产物的落盘目录(指令面板快照等), 默认 data",
            restart: true,
            ..base("connection", "data_dir", "数据目录", "text")
        },
        CoreField {
            options: &["webhook", "websocket"],
            restart: true,
            ..base("connection", "protocol", "协议模式", "select")
        },
        CoreField {
            desc: "至少选一个；全不选时启动使用默认订阅。频道事件(GUILD_*/CHANNEL_*/AT_MESSAGE_CREATE/MESSAGE_CREATE/DIRECT_MESSAGE_CREATE)要求协议为 websocket —— 平台要求机器人连着 WebSocket 才能往频道发消息；MESSAGE_CREATE 仅私域机器人有权限，公域只能收 @ 消息",
            options: INTENT_OPTIONS,
            restart: true,
            ..base("connection", "intents", "订阅事件", "multiselect")
        },
        CoreField {
            desc: "多选；取消「无前缀」后 confirm 等无前缀指令需带符号触发",
            options: PREFIX_OPTIONS,
            restart: true,
            ..base("connection", "prefixes", "指令前缀符号", "multiselect")
        },
        CoreField {
            desc: "留空保持原值; 修改后现有会话立即失效",
            hot: true,
            ..base("console", "admin_password", "管理密码", "secret")
        },
        CoreField {
            options: &["debug", "info", "warn", "error"],
            hot: true,
            ..base("logging", "log_level", "控制台日志级别", "select")
        },
        CoreField {
            hot: true,
            ..base("message", "global_markdown", "全局 Markdown 回复", "bool")
        },
        CoreField {
            desc: "开: 正文里每张 markdown 图片宽度顶到上限、高度按比例算出后封顶, 只缩不放; 超长图会被压进框里(比例会走样, 点开仍是原图), 换来的是一屏装得下、不再刷屏。关: 原样发出。**省略即开启**",
            hot: true,
            ..base(
                "message",
                "image_limits_enabled",
                "限制 Markdown 图片尺寸",
                "bool",
            )
        },
        CoreField {
            desc: "聊天气泡宽度; 350 约等于主流手机满宽, 小于它的图不放大。只能填 16..4096, 留空用默认值",
            placeholder: "350",
            unit: "px",
            hot: true,
            ..base("message", "image_max_width", "图片最大宽度(px)", "number")
        },
        CoreField {
            desc: "单张图在聊天窗里最多占多高; 高度超了就截到这个值, 宽度照样顶满。只能填 16..4096, 留空用默认值",
            placeholder: "438",
            unit: "px",
            hot: true,
            ..base("message", "image_max_height", "图片最大高度(px)", "number")
        },
        CoreField {
            desc: "每行一个业务错误码",
            hot: true,
            ..base("message", "retry_when", "消息重试错误码", "intlist")
        },
        CoreField {
            unit: "字节",
            hot: true,
            ..base(
                "message",
                "upload_threshold",
                "分片上传阈值(字节)",
                "number",
            )
        },
        CoreField {
            desc: "所有按钮板(含底部按钮)的缺省字号, 对应报文里的 keyboard.content.style.font_size。选「默认」= 不发这个字段, 用客户端默认大小; 选 small = 小号按钮。它只是缺省值: 插件自己指定过字号的键盘照旧",
            options: &["", "small"],
            hot: true,
            ..base("message", "keyboard_font_size", "按钮字号", "select")
        },
        CoreField {
            desc: "关掉后保留下面的配置, 但不再往消息里追加(含各场景专属按钮)",
            hot: true,
            ..base("footer", "footer_buttons_enabled", "启用底部按钮", "bool")
        },
        CoreField {
            desc: "每个 Markdown 消息末尾追加的底部按钮。一行 = 键盘上一行, 同一行用逗号分隔多个按钮; 每个按钮写 标签|类型|参数, 类型有 counter(计数)/command(指令)/link(链接)/repeat(复读: 把触发这条回复的原消息填进用户输入框)/image(图片: 点击把那一段 URL 的图发出来)/callback(回调: 点击转给参数里那个回调 id, 发什么由注册它的插件决定), 选项跟在参数后: ; k=v。style 可选 0 灰线 / 1 蓝线 / 2 灰底 / 3 蓝底 / 4 文字(不写就是这个)",
            placeholder: "+1 | repeat , 文档 | link | https://github.com/KasumiYuku/Aurorix\n帮助 | command | /help ; style=1",
            hot: true,
            ..base("footer", "footer_buttons", "底部按钮", "textlist")
        },
        CoreField {
            desc: "只挂在群消息上; 留空则沿用上面的「底部按钮」。语法与「底部按钮」完全一致",
            placeholder: "+1 | repeat , 帮助 | command | /help",
            hot: true,
            ..base("footer", "footer_buttons_group", "群聊底部按钮", "textlist")
        },
        CoreField {
            desc: "只挂在好友单聊消息上; 留空则沿用上面的「底部按钮」。语法同上",
            hot: true,
            ..base("footer", "footer_buttons_c2c", "单聊底部按钮", "textlist")
        },
        CoreField {
            desc: "只挂在子频道消息上; 留空则沿用上面的「底部按钮」。频道里「复读」按钮能不能用取决于平台, 配之前建议先在测试频道点一次",
            hot: true,
            ..base(
                "footer",
                "footer_buttons_channel",
                "频道底部按钮",
                "textlist",
            )
        },
        CoreField {
            desc: "只挂在频道私信里; 留空则沿用上面的「底部按钮」。语法同上",
            hot: true,
            ..base("footer", "footer_buttons_dm", "频道私信底部按钮", "textlist")
        },
        CoreField {
            desc: "收到新人进群事件时发欢迎。能用事件 ID 被动回复, 不占主动消息额度、不受群授权限制。",
            hot: true,
            ..base("member", "welcome_enabled", "新人进群欢迎", "bool")
        },
        CoreField {
            desc: "一行一个候选, 发送时随机挑一行; 留空则不发。消息第一行是 ## + 圆形头像 + @进群成员, 文案从第二行开始。",
            placeholder: "欢迎来到这里，请随意一点，也请温柔一点。\n欢迎加入，愿你在这里玩得开心。",
            hot: true,
            ..base("member", "welcome_messages", "新人进群文案", "textlist")
        },
        CoreField {
            desc: "收到成员退群事件时发提示。退群不支持被动回复, 只能主动推送; 群需开启「允许主动消息」才会发出。",
            hot: true,
            ..base("member", "farewell_enabled", "成员退群提示", "bool")
        },
        CoreField {
            desc: "一行一个候选; 留空则不发。退群的人已不在群里、@ 解析不到, 所以只有头像 + 文案。",
            placeholder: "有人离开了群聊，祝之后一切顺利。\n短暂同行也很好，下次有缘再见。",
            hot: true,
            ..base("member", "farewell_messages", "成员退群文案", "textlist")
        },
        CoreField {
            desc: "原样追加在欢迎文案后面, 支持换行、引用块(行首写 >)和内联指令标签。标签里的指令照人话写, 平台要求的 urlencode 由框架代劳 —— 自己先编一遍反而会双重编码。留空则不加",
            placeholder: "> <qqbot-cmd-input text=\"/p\" show=\"pgr菜单\" /> <qqbot-cmd-input text=\"/a\" show=\"arc菜单\" />",
            hot: true,
            ..base("member", "welcome_extra", "新人进群附加内容", "textarea")
        },
        CoreField {
            desc: "语法与上面「底部按钮」完全一致; 只挂在这条欢迎消息上, 不跟全局按钮混, 计数也各算各的。留空则不挂键盘",
            placeholder: "帮助 | command | /help ; style=1",
            hot: true,
            ..base("member", "welcome_buttons", "新人进群底部按钮", "textlist")
        },
        CoreField {
            desc: "同「新人进群附加内容」, 追加在退群文案后面, 同样不用自己编码。留空则不加",
            hot: true,
            ..base("member", "farewell_extra", "成员退群附加内容", "textarea")
        },
        CoreField {
            desc: "语法同「底部按钮」; 只挂在这条退群提示上。留空则不挂键盘",
            hot: true,
            ..base("member", "farewell_buttons", "成员退群底部按钮", "textlist")
        },
    ]
}

pub fn core_spec(key: &str) -> Option<CoreField> {
    core_specs().into_iter().find(|spec| spec.key == key)
}

pub fn field_value(config: &AppConfig, key: &str) -> (Value, bool) {
    match key {
        "port" => (json!(config.port), false),
        "listen_addr" => (json!(config.listen_addr()), false),
        "appid" => (json!(config.appid), false),
        "secret" => (json!(""), !config.secret.is_empty()),
        "proxy" => (json!(config.proxy), false),
        "gateway_url" => (json!(config.gateway_url.as_deref().unwrap_or("")), false),
        "database" => (json!(config.database), false),
        "data_dir" => (json!(config.data_dir()), false),
        "admin_password" => (json!(""), !config.admin_password.is_empty()),
        "protocol" => (json!(config.protocol()), false),
        "intents" => (json!(config.intents()), false),
        "prefixes" => (json!(display_prefixes(&config.prefixes())), false),
        "log_level" => (json!(config.log_level()), false),
        "bot_maintenance" => (json!(config.bot_maintenance.unwrap_or(false)), false),
        "bot_maintenance_message" => (
            json!(config.bot_maintenance_message.as_deref().unwrap_or("")),
            false,
        ),
        "global_markdown" => (json!(config.global_markdown.unwrap_or(false)), false),
        "image_limits_enabled" => (json!(config.image_limits_enabled.unwrap_or(true)), false),
        "image_max_width" => (json!(config.image_max_width()), false),
        "image_max_height" => (json!(config.image_max_height()), false),
        "retry_when" => (json!(config.retry_when()), false),
        "upload_threshold" => (json!(config.upload_threshold()), false),
        "footer_buttons" => (
            json!(config.footer_buttons.clone().unwrap_or_default()),
            false,
        ),
        "footer_buttons_group" => (
            json!(config.footer_buttons_group.clone().unwrap_or_default()),
            false,
        ),
        "footer_buttons_c2c" => (
            json!(config.footer_buttons_c2c.clone().unwrap_or_default()),
            false,
        ),
        "footer_buttons_channel" => (
            json!(config.footer_buttons_channel.clone().unwrap_or_default()),
            false,
        ),
        "footer_buttons_dm" => (
            json!(config.footer_buttons_dm.clone().unwrap_or_default()),
            false,
        ),
        "keyboard_font_size" => (
            json!(config.keyboard_font_size.as_deref().unwrap_or("")),
            false,
        ),
        "footer_buttons_enabled" => (json!(config.footer_buttons_enabled.unwrap_or(true)), false),
        "welcome_enabled" => (json!(config.welcome_enabled.unwrap_or(true)), false),
        "welcome_messages" => (
            json!(config.welcome_messages.clone().unwrap_or_default()),
            false,
        ),
        "farewell_enabled" => (json!(config.farewell_enabled.unwrap_or(true)), false),
        "farewell_messages" => (
            json!(config.farewell_messages.clone().unwrap_or_default()),
            false,
        ),
        "welcome_extra" => (json!(config.welcome_extra.as_deref().unwrap_or("")), false),
        "welcome_buttons" => (
            json!(config.welcome_buttons.clone().unwrap_or_default()),
            false,
        ),
        "farewell_extra" => (json!(config.farewell_extra.as_deref().unwrap_or("")), false),
        "farewell_buttons" => (
            json!(config.farewell_buttons.clone().unwrap_or_default()),
            false,
        ),
        _ => (Value::Null, false),
    }
}

fn field_view(spec: &CoreField, config: &AppConfig) -> Value {
    let (value, set) = field_value(config, spec.key);
    let mut view = Map::new();
    view.insert("key".to_string(), json!(spec.key));
    view.insert("label".to_string(), json!(spec.label));
    if !spec.desc.is_empty() {
        view.insert("desc".to_string(), json!(spec.desc));
    }
    view.insert("kind".to_string(), json!(spec.kind));
    if !spec.options.is_empty() {
        view.insert("options".to_string(), json!(spec.options));
    }
    view.insert("hot".to_string(), json!(spec.hot));
    view.insert("restart".to_string(), json!(spec.restart));
    view.insert("value".to_string(), value);
    view.insert("set".to_string(), json!(set));
    if spec.dedicated {
        view.insert("dedicated".to_string(), json!(true));
    }
    if !spec.placeholder.is_empty() {
        view.insert("placeholder".to_string(), json!(spec.placeholder));
    }
    view.insert("group".to_string(), json!(spec.group));
    if !spec.unit.is_empty() {
        view.insert("unit".to_string(), json!(spec.unit));
    }
    Value::Object(view)
}

pub fn field_views(config: &AppConfig) -> Vec<Value> {
    core_specs()
        .iter()
        .map(|spec| field_view(spec, config))
        .collect()
}

pub fn group_views() -> Vec<Value> {
    config_groups()
        .iter()
        .map(|group| {
            let mut view = Map::new();
            view.insert("id".to_string(), json!(group.id));
            view.insert("label".to_string(), json!(group.label));
            if !group.desc.is_empty() {
                view.insert("desc".to_string(), json!(group.desc));
            }
            Value::Object(view)
        })
        .collect()
}

pub fn is_secret_placeholder(value: &Value) -> bool {
    match value.as_str() {
        Some(text) => text.is_empty() || text == SECRET_MASK,
        None => false,
    }
}

pub fn masked_values(config: &AppConfig) -> Map<String, Value> {
    let mut values = match serde_json::to_value(config) {
        Ok(Value::Object(map)) => map,
        _ => Map::new(),
    };
    for key in SECRET_KEYS {
        let set = values
            .get(*key)
            .and_then(Value::as_str)
            .map(|text| !text.is_empty())
            .unwrap_or(false);
        if set {
            values.insert((*key).to_string(), Value::String(SECRET_MASK.to_string()));
        } else {
            values.remove(*key);
        }
    }
    values
}

pub fn config_view(config: &AppConfig) -> Value {
    json!({
        "fields": field_views(config),
        "groups": group_views(),
    })
}

pub fn is_writable(key: &str) -> bool {
    WRITABLE_KEYS.contains(&key)
}

pub fn is_secret(key: &str) -> bool {
    SECRET_KEYS.contains(&key)
}

pub fn needs_restart(key: &str) -> bool {
    RESTART_KEYS.contains(&key)
}

pub fn is_hot(key: &str) -> bool {
    core_spec(key).map(|spec| spec.hot).unwrap_or(false)
}

pub fn display_prefixes(prefixes: &[String]) -> Vec<String> {
    prefixes
        .iter()
        .map(|prefix| {
            if prefix.is_empty() {
                NO_PREFIX.to_string()
            } else {
                prefix.clone()
            }
        })
        .collect()
}
