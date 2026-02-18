use teloxide::prelude::*;
use dotenv::dotenv;
use std::sync::Arc;
use std::collections::HashMap;
use tokio::sync::RwLock;


mod turbo;
mod anonymous;
mod storage;
mod broadcast;
mod responses;



use crate::turbo::send_fast;
use crate::anonymous::AnonymousManager;
use crate::storage::BlockMgr;
use crate::broadcast::BroadcastManager;
use crate::responses::*;

struct AnonForeverTracker {
    users: Arc<RwLock<HashMap<i64, bool>>>,
}
impl AnonForeverTracker {
    fn new() -> Self {
        Self { users: Arc::new(RwLock::new(HashMap::new())) }
    }
    async fn toggle(&self, uid: i64) -> bool {
        let mut users = self.users.write().await;
        let current = users.get(&uid).copied().unwrap_or(false);
        let new_state = !current;
        users.insert(uid, new_state);
        new_state}
        async fn is_enabled(&self, uid: i64) -> bool {
            let users = self.users.read().await;
            users.get(&uid).copied().unwrap_or(false)}
            async fn disable(&self, uid: i64) {
                let mut users = self.users.write().await;
                users.remove(&uid);
            }
        }
        #[tokio::main]
        async fn main() -> Result<(), Box<dyn std::error::Error>> {
            dotenv().ok();
            env_logger::init();
            println!("ВОЗМОЖНО РАБОТАЕТ");
            let bot = Bot::new(std::env::var("BOT_TOKEN").expect("BOT_TOKEN"));
            let owner = ChatId(std::env::var("OWNER_CHAT_ID").expect("OWNER_CHAT_ID").parse::<i64>()?);
            let token = std::env::var("BOT_TOKEN").expect("BOT_TOKEN");
            let anon = Arc::new(AnonymousManager::new());
            let block = Arc::new(BlockMgr::new());
            let bcast = Arc::new(BroadcastManager::new());
            let anon_frvr = Arc::new(AnonForeverTracker::new());
            let h = Update::filter_message()
            .branch(dptree::filter(|msg: Message| msg.successful_payment().is_some())
            .endpoint(|bot: Bot, msg: Message| async move {
                if let Some(p) = msg.successful_payment() {
                    let amt = p.total_amount / 100;
                    bot.send_message(msg.chat.id, format!("✅ Платеж на {} звезд получен!", amt)).await?;
                }
                Ok::<(), teloxide::RequestError>(())
            }))
            .branch(dptree::endpoint(move |bot: Bot, msg: Message| {
                let a = anon.clone();
                let b = block.clone();
                let bc = bcast.clone();
                let af = anon_frvr.clone();
                let o = owner;
                let t = token.clone();
                async move {
                    handle_msg(bot, msg, a, b, bc, af, o, t).await;
                    Ok::<(), teloxide::RequestError>(())
                }
            }));
            let pc = Update::filter_pre_checkout_query()
            .endpoint(|bot: Bot, q: PreCheckoutQuery| async move {
                bot.answer_pre_checkout_query(q.id, true).await?;
                Ok::<(), teloxide::RequestError>(())
            });
            let d = dptree::entry().branch(h).branch(pc);
            Dispatcher::builder(bot, d).build().dispatch().await;
            Ok(())
        }
        async fn handle_msg(
            bot: Bot,
            msg: Message,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            bcast: Arc<BroadcastManager>,
            anon_frvr: Arc<AnonForeverTracker>,
            owner: ChatId,
            token: String,
        ) {
            if msg.chat.id == owner {
                owner_handler(bot, msg, block, bcast, token).await;
                return;
            }
            let user = match msg.from.clone() {
                Some(u) => u,
                None => return,
            };
            let uid = user.id.0 as i64;
            if is_user_blocked(uid, &user.username, &block).await {
                let _ = bot.send_message(msg.chat.id, r_banned()).await;
                return;
            }
            bcast.add_user_with_username(uid, user.username.clone()).await;
            if let Some(text) = msg.text() {
                handle_text_message(&bot, &msg, text, uid, &user, anon, block, anon_frvr, owner, token).await;
                return;
            }
            if has_media(&msg) {
                handle_media_message(&bot, msg, uid, anon, block, bcast, anon_frvr, owner).await;
            }
        }
        async fn is_user_blocked(
            uid: i64,
            username: &Option<String>,
            block: &Arc<BlockMgr>,
        ) -> bool {
            if block.is_id_blocked(uid).await {
                return true;
            }
            if let Some(name) = username {
                if block.is_user_blocked(&format!("@{}", name)).await {
                    return true;
                }
            }
            false
        }
        async fn handle_text_message(
            bot: &Bot,
            msg: &Message,
            text: &str,
            uid: i64,
            user: &teloxide::types::User,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            anon_frvr: Arc<AnonForeverTracker>,
            owner: ChatId,
            token: String,
        ) {
            if text.starts_with('/') {
                user_cmd(bot, msg, text, anon, block, anon_frvr, token).await;
            }
            else {
                if anon_frvr.is_enabled(uid).await {
                    send_anon_text(bot, msg.chat.id, text, uid, &anon, &block, owner, &token).await;
                }
                else {
                    send_normal_text(bot, msg.chat.id, text, user, owner, &token).await;
                }
            }
}
        async fn handle_media_message(
            bot: &Bot,
            msg: Message,
            uid: i64,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            bcast: Arc<BroadcastManager>,
            anon_frvr: Arc<AnonForeverTracker>,
            owner: ChatId,
        ) {
            if anon_frvr.is_enabled(uid).await {
                user_media_anon(bot, msg, uid, anon, block, owner).await;
            }
            else {
                user_media_normal(bot, msg, uid, anon, block, bcast, owner).await;
            }
        }
        async fn send_anon_text(
            bot: &Bot,
            cid: ChatId,
            text: &str,
            uid: i64,
            anon: &Arc<AnonymousManager>,
            block: &Arc<BlockMgr>,
            owner: ChatId,
            token: &str,
        )
        {
            let code = anon.get_or_create_anonymous_code(uid).await;
            block.reg_anon(&format!("{:04}", code), uid).await;
            if block.is_anon_blocked(&format!("{:04}", code)).await {
                let _ = bot.send_message(cid, r_banned()).await;
                return;
            }
            let full = format!("💡 Анонимное сообщение от Анонима #{:04}:\n{}", code, text);
            send_fast(token, &owner.0.to_string(), &full);
            let _ = bot.send_message(cid, format!("{}! Код: #{:04}", r_sent_anon(), code)).await;
        }
        async fn send_normal_text(
            bot: &Bot,
            cid: ChatId,
            text: &str,
            user: &teloxide::types::User,
            owner: ChatId,
            token: &str,
        )
        {
            let name = user.username.as_ref()
            .map(|n| format!("@{}", n))
            .unwrap_or_else(|| "Неизвестный".to_string());
            let txt = format!("💡 Сообщение от пользователя:\n{}\n\n👤 {}", text, name);
            send_fast(token, &owner.0.to_string(), &txt);
            let _ = bot.send_message(cid, r_sent_norm()).await;
        }
        async fn user_media_anon(
            bot: &Bot,
            msg: Message,
            uid: i64,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            owner: ChatId,
        )
        {
            let code = anon.get_or_create_anonymous_code(uid).await;
            block.reg_anon(&format!("{:04}", code), uid).await;
            if block.is_anon_blocked(&format!("{:04}", code)).await {
                let _ = bot.send_message(msg.chat.id, r_banned()).await;
                return;
            }
            let mt = media_type(&msg);
            let cap = msg.caption().unwrap_or("(без подписи)");
            let full = format!("{} от Анонима #{:04}\n💡 Подпись: {}", mt, code, cap);
            send_media(bot, owner, &msg, &full).await;
            let _ = bot.send_message(msg.chat.id, format!("{}! Код: #{:04}", r_sent_anon(), code)).await;
        }
        async fn user_media_normal(
            bot: &Bot,
            msg: Message,
            uid: i64,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            bcast: Arc<BroadcastManager>,
            owner: ChatId,
        ){
            let cap = msg.caption().map(|c| c.to_string());
            if let Some(c) = &cap {
                if c.trim().starts_with("noone") {
                    anon_media(bot, &msg, c, uid, anon, block, owner).await;
                    return;
                }
            }
            norm_media(bot, &msg, bcast, owner).await;
        }
        fn has_media(msg: &Message) -> bool {
            msg.animation().is_some() ||
            msg.audio().is_some() ||
            msg.document().is_some() ||
            msg.photo().is_some() ||
            msg.sticker().is_some() ||
            msg.video().is_some() ||
            msg.video_note().is_some() ||
            msg.voice().is_some()
        }
        fn media_type(msg: &Message) -> &'static str {
            if msg.video_note().is_some() {
                "📹 Кружок"
            }
            else if msg.voice().is_some() {
                "🎤 Голосовое"
            }
            else if msg.video().is_some() {
                "🎥 Видео"
            }
            else if msg.photo().is_some() {
                "📷 Фото"
            }
            else if msg.document().is_some() {
                "📄 Файл"
            }
            else if msg.audio().is_some() {
                "🎵 Аудио"
            }
            else if msg.animation().is_some() {
                "🔄 GIF"
            }
            else {
                "📎 Медиа"
            }
        }
        async fn anon_media(
            bot: &Bot,
            msg: &Message,
            cap: &str,
            uid: i64,
            anon: Arc<AnonymousManager>,
            block: Arc<BlockMgr>,
            owner: ChatId,
        )
        {
            let parts: Vec<&str> = cap.splitn(2, ' ').collect();
            if parts.len() < 2 || parts[1].trim().is_empty() {
                let _ = bot.send_message(msg.chat.id, r_noone_empty()).await;
                return;
            }
            let text = parts[1].trim();
            let code = anon.get_or_create_anonymous_code(uid).await;
            block.reg_anon(&format!("{:04}", code), uid).await;
            if block.is_anon_blocked(&format!("{:04}", code)).await {
                let _ = bot.send_message(msg.chat.id, r_banned()).await;
                return;
            }
            let mt = media_type(msg);
            let cap_text = if text.is_empty() {
                "(без подписи)"
            }
            else {
                text
            };
            let full = format!(
                "{} от Анонима #{:04}\n💡 Подпись: {}",
                mt, code, cap_text
            );
            send_media(bot, owner, msg, &full).await;
            let _ = bot.send_message(
                msg.chat.id,
                format!("{}! Код: #{:04}", r_sent_anon(), code)
            ).await;
        }
        async fn norm_media(
            bot: &Bot,
            msg: &Message,
            bcast: Arc<BroadcastManager>,
            owner: ChatId,
        )
        {
            let user = match &msg.from {
                Some(u) => u,
                None => return,
            };
            let uid = user.id.0 as i64;
            bcast.add_user_with_username(uid, user.username.clone()).await;
            let name = user.username.as_ref()
            .map(|n| format!("@{}", n))
            .unwrap_or_else(|| "Неизвестный".to_string());
            let mt = media_type(msg);
            let cap = msg.caption().unwrap_or("(без подписи)");
            let full = format!(
                "{} от пользователя:\n{}\n\n👤 {}",
                mt, cap, name
            );
            send_media(bot, owner, msg, &full).await;
            let _ = bot.send_message(msg.chat.id, r_sent_norm()).await;
        }
        async fn send_media(
            bot: &Bot,
            dst: ChatId,
            msg: &Message,
            cap: &str
        )
        {
            if let Some(photo) = msg.photo() {
                let file = photo.last().unwrap().file.id.clone();
                let _ = bot.send_photo(
                    dst,
                    teloxide::types::InputFile::file_id(file)
                ).caption(cap).await;}
                else if let Some(video) = msg.video() {
                    let file = video.file.id.clone();
                    let _ = bot.send_video(
                        dst,
                        teloxide::types::InputFile::file_id(file)
                    ).caption(cap).await;}
                    else if let Some(doc) = msg.document() {
                        let file = doc.file.id.clone();
                        let _ = bot.send_document(
                            dst,
                            teloxide::types::InputFile::file_id(file)
                        ).caption(cap).await;
                    }
                    else if let Some(audio) = msg.audio() {
                        let file = audio.file.id.clone();
                        let _ = bot.send_audio(
                            dst,
                            teloxide::types::InputFile::file_id(file)
                        ).caption(cap).await;
                    }
                    else if let Some(anim) = msg.animation() {
                        let file = anim.file.id.clone();
                        let _ = bot.send_animation(
                            dst,
                            teloxide::types::InputFile::file_id(file)
                        ).caption(cap).await;}
                        else if let Some(voice) = msg.voice() {
                            let file = voice.file.id.clone();
                            let _ = bot.send_voice(
                                dst,
                                teloxide::types::InputFile::file_id(file)
                            ).caption(cap).await;
                        }
                        else if let Some(video_note) = msg.video_note() {
                            let file = video_note.file.id.clone();
                            let _ = bot.send_video_note(
                                dst,
                                teloxide::types::InputFile::file_id(file)
                            ).await;
                        }
                        else if let Some(sticker) = msg.sticker() {
                            let file = sticker.file.id.clone();
                            let _ = bot.send_sticker(
                                dst,
                                teloxide::types::InputFile::file_id(file)
                            ).await;
                        }
                    }
                    async fn user_cmd(
                        bot: &Bot,
                        msg: &Message,
                        text: &str,
                        anon: Arc<AnonymousManager>,
                        block: Arc<BlockMgr>,
                        anon_frvr: Arc<AnonForeverTracker>,
                        token: String,
                    )
                    {
                        let parts: Vec<&str> = text.splitn(2, ' ').collect();
                        let cmd = parts[0].trim_start_matches('/');
                        match cmd {
                            "start" => {
                                cmd_start(bot, msg).await;
                            },
                            "help" => {
                                cmd_help(bot, msg).await;
                            },
                            "noone" => {
                                cmd_noone(bot, msg, &parts, anon, block, token).await;
                            },
                            "noone_frvr" => {
                                cmd_noone_frvr(bot, msg, anon_frvr, anon, block).await;
                            },
                            "donate" => {
                                cmd_donate(bot, msg).await;
                            },
                            "code" => {
                                cmd_code(bot, msg, anon).await;
                            },
                            _ => {},
                        }
                    }
                    async fn cmd_start(bot: &Bot, msg: &Message) {
                        let resp = format!(
                            "👋 Привет! Пиши сюда свои сообщения владельцу бота.\n\n\
        💡 Команды:\n\
        /help - помощь\n\
        /noone текст - анонимное сообщение\n\
        /noone_frvr - режим \"всегда анонимно\"\n\
        /code - узнать свой анон-код\n\
        /donate - поддержать автора\n\n\
        ✨ {}",
                            r_interesting()
                        );
                        let _ = bot.send_message(msg.chat.id, resp).await;
                    }
                    async fn cmd_help(bot: &Bot, msg: &Message) {
                        let help_text = "📖 Помощь:\n\n\
        📝 Просто напиши текст - он придет владельцу\n\
        🔒 /noone текст - отправить анонимно\n\
        🎨 Медиа с подписью 'noone текст' - анонимное медиа\n\
        🔁 /noone_frvr - включить/выключить режим \"всегда анонимно\"\n\
        🔑 /code - узнать свой анон-код\n\
        💰 /donate - поддержать проект\n\n\
        Просто пиши цифру (1-10000) чтобы задонатить!";
                        let _ = bot.send_message(msg.chat.id, help_text).await;
                    }
                    async fn cmd_noone(
                        bot: &Bot,
                        msg: &Message,
                        parts: &[&str],
                        anon: Arc<AnonymousManager>,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        if parts.len() < 2 || parts[1].trim().is_empty() {
                            let _ = bot.send_message(msg.chat.id, r_noone_empty()).await;
                            return;
                        }
                        let user = match msg.from.as_ref() {
                            Some(u) => u,
                            None => return,
                        };
                        let uid = user.id.0 as i64;
                        let text = parts[1].trim();
                        let code = anon.get_or_create_anonymous_code(uid).await;
                        block.reg_anon(&format!("{:04}", code), uid).await;
                        if block.is_anon_blocked(&format!("{:04}", code)).await {
                            let _ = bot.send_message(msg.chat.id, r_banned()).await;
                            return;
                        }
                        let owner_id = std::env::var("OWNER_CHAT_ID")
                        .unwrap()
                        .parse::<i64>()
                        .unwrap();
                        let owner = ChatId(owner_id);
                        let full = format!(
                            "💡 Анонимное сообщение от Анонима #{:04}:\n{}",
                            code, text
                        );
                        send_fast(&token, &owner.0.to_string(), &full);
                        let _ = bot.send_message(
                            msg.chat.id,
                            format!("{}! Код: #{:04}", r_sent_anon(), code)
                        ).await;
                    }
                    async fn cmd_noone_frvr(
                        bot: &Bot,
                        msg: &Message,
                        anon_frvr: Arc<AnonForeverTracker>,
                        anon: Arc<AnonymousManager>,
                        block: Arc<BlockMgr>,
                    ) {
                        let user = match msg.from.as_ref() {
                            Some(u) => u,
                            None => return,
                        };
                        let uid = user.id.0 as i64;
                        let enabled = anon_frvr.toggle(uid).await;
                        if enabled {
                            let code = anon.get_or_create_anonymous_code(uid).await;
                            block.reg_anon(&format!("{:04}", code), uid).await;
                            let resp = format!(
                                "🔁 Режим \"всегда анонимно\" ВКЛЮЧЕН!\n\n\
            Все твои сообщения теперь будут отправляться анонимно.\n\
            Твой код: #{:04}\n\n\
            Чтобы выключить, напиши /noone_frvr снова.",
                                code
                            );
                            let _ = bot.send_message(msg.chat.id, resp).await;
                        }
                        else {
                            anon_frvr.disable(uid).await;
                            let resp = "✅ Режим \"всегда анонимно\" ВЫКЛЮЧЕН!\n\n\
            Теперь твои сообщения будут отправляться с твоим именем.";
                            let _ = bot.send_message(msg.chat.id, resp).await;
                        }
                    }
                    async fn cmd_donate(bot: &Bot, msg: &Message) {
                        let donate_text = "⭐️ Поддержать проект:\n\n\
        Просто напиши число от 1 до 10000 - это количество звезд Telegram.\n\n\
        Спасибо за поддержку! 🙏";
                        let _ = bot.send_message(msg.chat.id, donate_text).await;
                    }
                    async fn cmd_code(
                        bot: &Bot,
                        msg: &Message,
                        anon: Arc<AnonymousManager>
                    ) {
                        if let Some(user) = msg.from.as_ref() {
                            let uid = user.id.0 as i64;
                            if let Some(code) = anon.get_anonymous_code(uid).await {
                                let _ = bot.send_message(
                                    msg.chat.id,
                                    format!("🔑 Твой код: #{:04}", code)
                                ).await;
                            }
                            else {
                                let no_code_text = "❌ Сначала отправь хотя бы одно \
                анонимное сообщение через /noone";
                                let _ = bot.send_message(msg.chat.id, no_code_text).await;
                            }
                        }
                    }
                    async fn owner_handler(
                        bot: Bot,
                        msg: Message,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        let cid = msg.chat.id;
                        if let Some(reply) = msg.reply_to_message() {
                            let reply_clone = reply.clone();
                            reply_dialogue(bot, msg, reply_clone, block, bcast, token).await;
                            return;
                        }
                        if let Some(text) = msg.text() {
                            owner_text_cmd(bot, cid, text, block, bcast, token).await;
                        }
                        else if has_media(&msg) {
                            owner_media_cmd(bot, msg, block, bcast, token).await;
                        }
                    }
                    async fn owner_text_cmd(
                        bot: Bot,
                        cid: ChatId,
                        text: &str,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        let parts: Vec<&str> = text.splitn(2, ' ').collect();
                        let cmd = parts[0].trim_start_matches('/');
                        let args = if parts.len() > 1 {
                            parts[1].trim()
                        }
                        else {
                            ""
                        };
                        match cmd {
                            "start" => {
                                owner_cmd_start(bot, cid).await;
                            },
                            "status" => {
                                if !args.is_empty() {
                                    status_cmd(bot, cid, args, block, bcast).await;
                                }
                                else {
                                    let _ = bot.send_message(
                                        cid,
                                        "❌ /status @user или /status anonXXXX или /status ID"
                                    ).await;
                                }
                            },
                            "bcast" => {
                                if !args.is_empty() {
                                    bcast_cmd(bot, cid, args, bcast, token).await;
                                }
                                else {
                                    let _ = bot.send_message(
                                        cid,
                                        "❌ /bcast текст (или отправь с медиа)"
                                    ).await;
                                }
                            },
                            "pm" => {
                                if !args.is_empty() {
                                    pm_cmd(bot, cid, args, block, bcast, token).await;
                                }
                                else {
                                    let _ = bot.send_message(
                                        cid,
                                        "❌ /pm @user текст или /pm anonXXXX текст (или с медиа)"
                                    ).await;
                                }
                            },
                            "block" => {
                                if !args.is_empty() {
                                    block_cmd(bot, cid, args, block, token).await;
                                }
                                else {
                                    let _ = bot.send_message(
                                        cid,
                                        "❌ /block @user или /block anonXXXX"
                                    ).await;
                                }
                            },
                            "unblock" => {
                                if !args.is_empty() {
                                    unblock_cmd(bot, cid, args, block, token).await;
                                }
                                else {
                                    let _ = bot.send_message(
                                        cid,
                                        "❌ /unblock @user или /unblock anonXXXX"
                                    ).await;
                                }
                            },
                            "blockall" => {
                                block_all_cmd(bot, cid, bcast, block, token).await;
                            },
                            _ => {},
                        }
                    }
                    async fn owner_cmd_start(bot: Bot, cid: ChatId) {
                        let help_text = "👑 Админ-панель:\n\n\
        /status @user или anonXXXX - инфо о юзере\n\
        /pm @user текст - личное сообщение\n\
        /bcast текст - рассылка всем\n\
        /block @user или anonXXXX - заблокировать\n\
        /unblock @user или anonXXXX - разблокировать\n\
        /blockid ID - блок по ID\n\
        /unblockid ID - разблок по ID\n\
        /blockall - заблокировать всех\n\n\
        Ответь на сообщение чтобы ответить юзеру\n\
        Отправь медиа с /pm или /bcast в подписи";
                        let _ = bot.send_message(cid, help_text).await;
                    }
                    async fn reply_dialogue(
                        bot: Bot,
                        msg: Message,
                        reply: Message,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        let cid = msg.chat.id;
                        let reply_text = reply.text().unwrap_or("");
                        let target = extract_target_from_reply(reply_text);
                        let target = match target {
                            Some(t) => t,
                            None => {
                                let _ = bot.send_message(
                                    cid,
                                    "ℹ️ Ответь на сообщение анонима или юзера"
                                ).await;
                                return;
                            }
                        };
                        let uid = resolve_target_to_uid(&target, &block, &bcast).await;
                        let uid = match uid {
                            Some(id) => id,
                            None => {
                                let _ = bot.send_message(cid, r_not_found()).await;
                                return;
                            }
                        };
                        if let Some(text) = msg.text() {
                            send_reply_text(bot, cid, uid, &target, text, &token).await;
                        }
                        else if has_media(&msg) {
                            send_reply_media(bot, cid, uid, &target, &msg).await;
                        }
                    }
                    fn extract_target_from_reply(text: &str) -> Option<String> {
                        if text.contains("Анонима #") {
                            text.find("Анонима #")
                            .and_then(|pos| {
                                text.get(
                                    pos + "Анонима #".len()..
                                    pos + "Анонима #".len() + 4
                                )
                            })
                            .map(|code| format!("anon{}", code))
                        }
                        else if text.contains("👤 @") {
                            text.find("👤 @")
                            .and_then(|pos| text.get(pos + "👤 @".len()..))
                            .map(|rest| {
                                let username: String = rest.chars()
                                .take_while(|c| c.is_alphanumeric() || *c == '_')
                                .collect();
                                format!("@{}", username)
                            })
                        }
                        else {
                            None
                        }
                    }
                    async fn resolve_target_to_uid(
                        target: &str,
                        block: &Arc<BlockMgr>,
                        bcast: &Arc<BroadcastManager>,
                    ) -> Option<i64> {
                        if target.starts_with("anon") {
                            let code = &target[4..8];
                            block.get_by_anon(code).await
                        }
                        else if target.starts_with('@') {
                            bcast.get_user_by_username(&target[1..]).await
                        }
                        else {
                            None
                        }
                    }
                    async fn send_reply_text(
                        bot: Bot,
                        cid: ChatId,
                        uid: i64,
                        target: &str,
                        text: &str,
                        token: &str,
                    ) {
                        let full = format!("💬 {}:\n{}", r_from_author(), text);
                        send_fast(token, &uid.to_string(), &full);
                        let _ = bot.send_message(
                            cid,
                            format!("✅ {} отправлено!", target)
                        ).await;
                    }
                    async fn send_reply_media(
                        bot: Bot,
                        cid: ChatId,
                        uid: i64,
                        target: &str,
                        msg: &Message,
                    ) {
                        let cap = msg.caption().unwrap_or("");
                        let full = format!("💬 {}:\n{}", r_from_author(), cap);
                        send_media(&bot, ChatId(uid), msg, &full).await;
                        let _ = bot.send_message(
                            cid,
                            format!("✅ {} (медиа) отправлено!", target)
                        ).await;
                    }
                    async fn owner_media_cmd(
                        bot: Bot,
                        msg: Message,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                        _token: String,
                    ) {
                        let cid = msg.chat.id;
                        let cap = msg.caption().map(|c| c.to_string()).unwrap_or_default();
                        if cap.starts_with("/pm ") {
                            owner_media_pm(bot, cid, msg, &cap, block, bcast).await;
                        }
                        else if cap.starts_with("/bcast ") {
                            owner_media_bcast(bot, cid, msg, &cap, bcast).await;
                        }
                    }
                    async fn owner_media_pm(
                        bot: Bot,
                        cid: ChatId,
                        msg: Message,
                        cap: &str,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                    ) {
                        let args = &cap[4..].trim();
                        let parts: Vec<&str> = args.splitn(2, ' ').collect();
                        if parts.is_empty() {
                            let _ = bot.send_message(
                                cid,
                                "❌ /pm @user или /pm anonXXXX (в подписи)"
                            ).await;
                            return;
                        }
                        let target = parts[0];
                        let text = if parts.len() > 1 {
                            parts[1]
                        }
                        else {
                            ""
                        };
                        let uid = if target.starts_with("anon") && target.len() == 8 {
                            let code = &target[4..8];
                            block.get_by_anon(code).await
                        }
                        else if target.starts_with('@') {
                            bcast.get_user_by_username(&target[1..]).await
                        }
                        else {
                            None
                        };
                        if let Some(uid) = uid {
                            let full = if text.is_empty() {
                                format!("💬 {}", r_from_author())
                            }
                            else {
                                format!("💬 {}:\n{}", r_from_author(), text)
                            };
                            send_media(&bot, ChatId(uid), &msg, &full).await;
                            let _ = bot.send_message(
                                cid,
                                format!("✅ {} (медиа) отправлено!", target)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(cid, r_not_found()).await;
                        }
                    }
                    async fn owner_media_bcast(
                        bot: Bot,
                        cid: ChatId,
                        msg: Message,
                        cap: &str,
                        bcast: Arc<BroadcastManager>,
                    ) {
                        let text = &cap[7..].trim();
                        let list = bcast.get_broadcast_list().await;
                        if list.is_empty() {
                            let _ = bot.send_message(cid, "📭 Нет юзеров").await;
                            return;
                        }
                        let _ = bot.send_message(
                            cid,
                            format!("📢 Рассылка (медиа) для {} юзеров...", list.len())
                        ).await;
                        let full = if text.is_empty() {
                            "📢 Рассылка от владельца".to_string()
                        }
                        else {
                            format!("📢 Рассылка от владельца:\n\n{}", text)
                        };
                        let mut sent = 0;
                        for uid in list {
                            send_media(&bot, ChatId(uid), &msg, &full).await;
                            sent += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        let _ = bot.send_message(
                            cid,
                            format!("✅ Отправлено (медиа): {}", sent)
                        ).await;
                    }
                    async fn status_cmd(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                    ) {
                        if target.starts_with("anon") && target.len() == 8 {
                            show_anon_status(bot, owner, target, block).await;
                        }
                        else if target.starts_with('@') {
                            show_user_status(bot, owner, target, block, bcast).await;
                        }
                        else if let Ok(uid) = target.parse::<i64>() {
                            show_id_status(bot, owner, uid, block, bcast).await;
                        }
                        else {
                            let _ = bot.send_message(
                                owner,
                                "❌ Укажи @user, anonXXXX или ID"
                            ).await;
                        }
                    }
                    async fn show_anon_status(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                    ) {
                        let code = &target[4..8];
                        if let Some(_) = block.get_by_anon(code).await {
                            let blocked = block.is_anon_blocked(code).await;
                            let status = format!(
                                "📊 Аноним #{}\n\n🚫 Заблокирован: {}",
                                code, if blocked {"Да"} else {"Нет"}
                            );
                            let _ = bot.send_message(owner, status).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn show_user_status(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                    )
                    {
                        if let Some(uid) = bcast.get_user_by_username(&target[1..]).await {
                            let blocked = block.is_id_blocked(uid).await;
                            let list = bcast.get_broadcast_list().await;
                            let in_list = list.contains(&uid);
                            let status = format!(
                                "📊 Юзер {}\n\n🚫 Заблокирован: {}\n📢 В рассылке: {}",
                                target,
                                if blocked {"Да"}
                                else {"Нет"},
                                if in_list {"Да"}
                                else {"Нет"}
                            );
                            let _ = bot.send_message(owner, status).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn show_id_status(
                        bot: Bot,
                        owner: ChatId,
                        uid: i64,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                    ) {
                        let blocked = block.is_id_blocked(uid).await;
                        let list = bcast.get_broadcast_list().await;
                        let in_list = list.contains(&uid);
                        let status = format!(
                            "📊 ID: {}\n🚫 Заблокирован: {}\n📢 В рассылке: {}",
                            uid,
                            if blocked {"Да"}
                            else {"Нет"},
                            if in_list {"Да"}
                            else {"Нет"}
                        );
                        let _ = bot.send_message(owner, status).await;
                    }
                    async fn bcast_cmd(
                        bot: Bot,
                        owner: ChatId,
                        text: &str,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        let msg = format!("📢 Рассылка от владельца:\n\n{}", text);
                        let list = bcast.get_broadcast_list().await;
                        if list.is_empty() {
                            let _ = bot.send_message(owner, "📭 Нет юзеров").await;
                            return;
                        }
                        let _ = bot.send_message(
                            owner,
                            format!("📢 Рассылка для {} юзеров...", list.len())
                        ).await;
                        let mut sent = 0;
                        for uid in list {
                            send_fast(&token, &uid.to_string(), &msg);
                            sent += 1;
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                        }
                        let _ = bot.send_message(
                            owner,
                            format!("✅ Отправлено: {}", sent)
                        ).await;
                    }
                    async fn pm_cmd(
                        bot: Bot,
                        owner: ChatId,
                        args: &str,
                        block: Arc<BlockMgr>,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        let parts: Vec<&str> = args.splitn(2, ' ').collect();
                        if parts.len() < 2 {
                            let _ = bot.send_message(
                                owner,
                                "❌ /pm @user текст или /pm anonXXXX текст"
                            ).await;
                            return;
                        }
                        let target = parts[0];
                        let text = parts[1];
                        if target.starts_with("anon") && target.len() == 8 {
                            pm_to_anon(bot, owner, target, text, block, token).await;
                        }
                        else if target.starts_with('@') {
                            pm_to_user(bot, owner, target, text, bcast, token).await;
                        }
                        else {
                            let _ = bot.send_message(
                                owner,
                                "❌ Укажи @user или anonXXXX"
                            ).await;
                        }
                    }
                    async fn pm_to_anon(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        text: &str,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        let code = &target[4..8];
                        if let Some(uid) = block.get_by_anon(code).await {
                            let full = format!("💬 {}:\n{}", r_from_author(), text);
                            send_fast(&token, &uid.to_string(), &full);
                            let _ = bot.send_message(
                                owner,
                                format!("✅ Анониму #{} отправлено!", code)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn pm_to_user(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        text: &str,
                        bcast: Arc<BroadcastManager>,
                        token: String,
                    ) {
                        if let Some(uid) = bcast.get_user_by_username(&target[1..]).await {
                            let full = format!("💬 {}:\n{}", r_from_author(), text);
                            send_fast(&token, &uid.to_string(), &full);
                            let _ = bot.send_message(
                                owner,
                                format!("✅ @{} отправлено!", &target[1..])
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn block_cmd(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        if target.starts_with("anon") && target.len() == 8 {
                            block_anon_target(bot, owner, target, block, token).await;
                        }
                        else if target.starts_with('@') {
                            block_user_target(bot, owner, target, block).await;
                        }
                        else {
                            let _ = bot.send_message(
                                owner,
                                "❌ Укажи @user или anonXXXX"
                            ).await;
                        }
                    }
                    async fn block_anon_target(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        let code = &target[4..8];
                        if block.block_anon(code).await {
                            if let Some(uid) = block.get_by_anon(code).await {
                                send_fast(&token, &uid.to_string(), r_you_blocked());
                            }
                            let _ = bot.send_message(
                                owner,
                                format!("{} Аноним #{}", r_block_ok(), code)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn block_user_target(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                    ) {
                        if block.block_user(target).await {
                            let _ = bot.send_message(
                                owner,
                                format!("{} {}", r_block_ok(), target)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn unblock_cmd(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        if target.starts_with("anon") && target.len() == 8 {
                            unblock_anon_target(bot, owner, target, block, token).await;
                        }
                        else if target.starts_with('@') {
                            unblock_user_target(bot, owner, target, block).await;
                        }
                        else {
                            let _ = bot.send_message(
                                owner,
                                "❌ Укажи @user или anonXXXX"
                            ).await;
                        }
                    }
                    async fn unblock_anon_target(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                        token: String,
                    ) {
                        let code = &target[4..8];
                        if block.unblock_anon(code).await {
                            if let Some(uid) = block.get_by_anon(code).await {
                                send_fast(&token, &uid.to_string(), r_you_unblocked());
                            }
                            let _ = bot.send_message(
                                owner,
                                format!("{} Аноним #{}", r_unblock_ok(), code)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                    async fn unblock_user_target(
                        bot: Bot,
                        owner: ChatId,
                        target: &str,
                        block: Arc<BlockMgr>,
                    ) {
                        if block.unblock_user(target).await {
                            let _ = bot.send_message(
                                owner,
                                format!("{} {}", r_unblock_ok(), target)
                            ).await;
                        }
                        else {
                            let _ = bot.send_message(owner, r_not_found()).await;
                        }
                    }
                            async fn block_all_cmd(
                                bot: Bot,
                                owner: ChatId,
                                bcast: Arc<BroadcastManager>,
                                block: Arc<BlockMgr>,
                                token: String,
                            ) {
                                let list = bcast.get_broadcast_list().await;
                                let mut cnt = 0;
                                for uid in list {
                                    if block.block_id(uid).await {
                                        cnt += 1;
                                        send_fast(&token, &uid.to_string(), r_you_blocked());
                                        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    }
                                }
                                let _ = bot.send_message(
                                    owner,
                                    format!("🚫 Заблокировано {} юзеров", cnt)
                                ).await;
                            }
                            /* Как я себя чувствую:


                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓██████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▓▓▒▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓█████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓█████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░▒░░░░░▒▒▒░▒░░▒▒░░░░▒▒░░░░▒░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓█████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▒▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░▒░░░░░░░░░░░░░░░░░░░░░░░░▒▒▓▓▓▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓██████▓▓▓▓▓▓▓▒▓▒▒▓▓▓▒▒▒▓▒▒▒▒▒▒▒░░░▒░░▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░▒░▒░░░░░░▒░▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓█▓██████████▓▓▓███▓▓▓▓▓▓▓▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░▒▒▒▒░░░▒░▒▒▒▒░▒░▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓███████▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒░▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓█████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓█▓▓▓▓▓██████▓█▓▓▓▓▓▓▓▒▓▒▓▓▓▓▒▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓██████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓▓▓█▓█▓██▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒▒░▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓█████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▓▒▒▒▒▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓█████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▒▒▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▓▓▓▓▓▒▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓█▓██████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▒▓▒▓▒▓▓▓▓▓▓▓▓▓▒▒▓▒▒▒▒▒▒▒░▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓██████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▒▒▒▓▓▓▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓███████████████████▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▒▒▒▒▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓██████████████████▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▓▓▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░▒░▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓██████████████████▓▓▓▓▓▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒░▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓███████████████▓█▓▓▓▓▓▓▓▓▓██████▓▓▓▓▒▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▓▓▒▓▓▓▒▓▓▒▓▒▒▒▒▒▒▒▒░░▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓█████████████▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▒▒▒▒▒▒▒▒▓▒▒▒▒▓▓▒▒▓▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓████████████▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▓▒▒▒▒▒▒▒▒▒▒░░░░▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓███████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓█▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▓▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓█████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▒▓▓▓▓██▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒░░▒▒░░░░░░░▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓██████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓█▓▓█▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▓▒▒▒▒▒▒▒▒▒▒▒░▒▒▒▒▒▒▒▒▒▓▓▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓▓▓██▓█▓▓▓▓█▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▒▒▒▒▒▒▓▓▒▓▒▓▓▒▒▒▒▒▒▒▒▒░▒▒▒▓▓▓▓▓▓████▓▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓█▓█▓██▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓█▓▓▓▓▓▒▒▒▒▒▒▒▓▓▓▒▒▒▒▓▓▒▓▒▒▒▒▒▒▒▒▒▒▓▓▓▓████▓▓▓▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓██▓▓▒▓███▓█▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▒▓▒▒▒▓▒▓▒▒▒▒▒▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░▒▒▒░░░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓█████████▓▓▓▓▓▓▓▓▓▓█▓▓▓█▓▓▓▓███▓█▓▓█▓███▓▓▓▓▓▓▓▓▓█▓▓▓▓▓█████▓▓▓▓▓▓▓▒▓▒▓▓▒▒▒▒▒▒▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░▒▒▒▒▒▒▒▒░▒░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓████████▓▓▓▓▓▓▓▓▓▓█▓▓▓▓█▓▓▓█████▓██████▓▓▓▓▓▓▓█▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▓▓█▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░▒▒▒▒▓▓▓▓▓▒▒░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓█▓██████▓▓▓▓▓▓▓▓█▓▓▓▓▓▓█▓████▓▓▓▓▓▓██▓▓▓███▓▓▓▓▓█████▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒░▒▒▓▒▒▒▒▓▓▓▒▒▒▒▒░░░░░░░▒▒▒▒▒▒▓▓▓▓▓▓▒▒▒░░░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓████▓▓███▓██▓████▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▓▓▒▒▓▒▒▓▒▒▒▓▒▒▒▓▓████▓▒▒▒▒▒░▒▒▓▓▓▓▒▒░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▒▒░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓███▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓██████████▓█▓▓████████▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▓▒▒▒▒▒▒▓▓▓█████▒██▓█▓▒▓▒▒▒▒▒▒▒▓▒▒░░░░░▒▒▒▒▒▒▓▓▓▓██▓▓▓▓▓▓▒░░░░░░░░░░░░░░░▒▒▒▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓█▓██████▓█▓▓█▓█████████████▓██████████▓▓▓▓▓▓▓▓▒▒▒▒▓▒▓▒▒▒▓▒▒▒▒▒▒▒▒▒▓▓█▓███████░▒▓▓▒▓▒▒▒▒▒▒▒▒▒▒░░░▒▒▒▒▒▒▒▒▒▒▒░▒▒█▓▓▓▓▒░░░░░░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓██████▓█▓██▓█████████████████████▓█▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▓███▓░░░▒░▒▒▒▒▒▒▒▒▒▒▒░░░░░▒▒▒▒▒▓█████▓▓▒▓▒▒▒░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓██▓███▓▓▓█▓▓██████████████████████▓▓▓▓▓▓▓▓▒▒▓▓▓▒▒▒▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒▒░░░░░░░▒▒░░░▒▒▒▒▒▒▒░░░░░▒░▒▒▓██████▒█▓░▓▒░░░░░░░░░░░░░░░░░░░▒▒░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓██▓███▓▓▓█▓▓█████████████████████████▓▓▓▓▓▓▓▒▓▒▒▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒░░░░▒▒▒▒▒▒▒▒░░░░░░░░▒░▓███▓▒░██▒▓▒░░░░░░░░░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▒▓▓█████▓▓▓▓█▓▓▓█████▓▓████████████▓▓▓██▓▓█▓▓▓▓▓▓▒▓▓▓▒▒▒▒░░░▒▒▒▒▒▒░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░▒▒░░▒▒▓▒▒░░░░░░░░░░░░░░░░░░░▒▒░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓████████▓▓▓▓▓▓██████▓▓████████████▓▓▓█▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▒▒▒▒▒▒░░▒▒░░▒▒▒▒░░▒░▒▒▒▒▒▒▒░▒▒░▒▒▒▒▒▒▒▒▒░░░░░░░░▒▒▒▒░░░░░▒░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓█▓▓▓▓██████▓█▓██▓▓▓▓▒▒█████▓▓▓██████████▓▓▓▓▓█▓▓██▓▓▓▓▓████▓▓▒▒▒▒▒▒▒░░░░▒▒░▒▒▒▒▒▒▒▒▒▒▒░░░░░░░▒▒▒▒▒▒▒▒▒░░░░░▒▒▒░░░░░░░░░▓░░░░░░░░░░░░░▒░░▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▓██▓▓████████▓██▓▒▒▓▒▒▓████▓▓▓████████████▓▓▓███▓██▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒░░▒▒░▒░▒▒░░░░░░░░░▒░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░▒▒▒▒░░░░░░░░▒▒░░░░▒░░▒▒▒░░▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓███▓█▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓██▓██▓▒▒▒▒▓▓████▓▒▒▓███████████▓▓▓█▓▓▓▓▓█▓▓▓▓▓█▓▓▓▓▓▒▒▒▒░░░░░░░▒▒▒▒░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░▒▒▒▒▒░░░░░░░░░░░▒▒▒▒▒▒▒▒░▒░▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓██▓███▒▒▒▒▒▓█████▓▓▒▓████████████▓███▓▓▓▓█▓▓▓▓▓▓█▓▓▓▒▒▒░▒░▒▒░░▒░░░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░▒▒▒░░▒░░░░░░░░░▒▒▒▒▒▒▒▒▒░▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓█▓▓███▒▒▒▒▒▓██████▓▒▓▓▓▓█████████████▓▓▓▓▓▓█▓▓█▓▓▓▓▓▒▒▒░▒▒░░░░░░▒░░▒░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░▒▒░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓█▓▓▓██▒▒▒▒▒▓█████████▓▓▒▓███████████▓▓▓▓▓▓▓█▓██▓▓▓▓▓▒▒▒░▒░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▒▒▒▒░░░░░░░░░░░▒░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓███▓▒▒▒▒▓██████▓▓█▓▓▒▒▓███████████▓▓▓▓▓▓▓▓██▓▓▓▓▓▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▒▒▒░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒░░▒▓▓▓▓▓▓▓▒▒▒▒▒▒░░░░░░░░░▒░░░░░▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓█████▓▓▓▓▓██▓▓▒▒▒▒▓▓█████▓▓▓▓▓▒▒▒▓█████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▓▓▒▒▒░░░░░░░░░░░░░░▒▒░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒░▒░░▒▓▓▓▒▒▓▒▒▒░░▒░░░░░▒▒▒▒░░░░░░▒▒░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓██▓█▓▒▒▒▓███████▓▓▓▓▒▒▒▓███████████▓█▓▓▓▓▓█▓▓▓▓▓▓▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░▒▒▒▓▓▒▒▒▒▒▒▒▓▒▒▒░░░░░░░░░░░░░░░▒▒░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒░▓▓▓▓▒▒▒▒░░░░░░▒▒▓▒▒▒▒▒░▒▒▒▒░░░░░░░░░░░▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓█▓▓▓██▓▓██▓▒▒▓███████▓▓▓▓▒▒▒▓████▓█▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░▒▒▒▒▒▓▓▒▒▒░░▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▓▓▓▓▓▓▓▓▓▒▓▓▒▒▓▓▒▒▒▒▒▒▒▓▒▓▓▒▒▒▒▒░░░░▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░▓█▓▒░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓██▓▓▓▓▓▓█▓████▓▒▒▓████████▓▓▓▒▒▒▓███████▓███▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░▒▒▒░▒▒▓▓▓▓▓▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▒▒▒▒▓▓▒▓█▓▒▒▒▒░░░▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░▒░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓█▓▓▓▓█▓▓▓█████▓▒▒▒████████▓▓▒▒▒▓███████▓██▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░▒░░░░▒▒▓▓▓▓▓█████▓▓▒▒▒▒▒▒▒▒▒▒▒░▒░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▒▒▒▒▒▓▓▓▓▓▓▒▒▒▓▒▓▓▓▓▒▒▒▒░░▒▒▒▒▓▒▒▒▒▒▒▒░░░░░░░░▒▒▒▒▒▒▒▒▒░░░░░░▒▒░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓███████▓▓▓▓█████████▓▓▓▓█████████▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▓▒▒▒▒▒░░▒░░░▒▒░░░░░░▒▓▓▓▓▓▓▓▓▓███▓▓▒▒▒▒▒▓▓▒░▒▒▒░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▒▒░░░░▒▒▒▓▒▒▒▒▒░░░░░░░░░░▒▒▒▒▒▒▒░░░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓███████████████▓██████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░▒▒▒▒▓▒▓▓███▓▓▓▓▓▓▒▒░░░▒▒▒▒▒░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓▓▓▓▒▒▒▒▒▓▒▒▒▒▓▒▒░░░░░▒░▒▒▒▒▓▓▒░░░░░░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████▓▓▓▓█▓▓█▓▓▓▓▓▓▓▒▒▒▒▓▒▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒░▒░▒░▒▒▒▓▒▓▓▓▓▓▓▒░░░░░░░▒▒▒▒░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▓▓███▓▓▓██▓▒▒▒▒▒▓▓▓▓▓█▓▓▒▓▓▓▒▒▒▓▓▓▒▒▒░░░░░▒▒▓██▒░░░▒▒▒▒░░░░░░░░░░░░░░█▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▓▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒░░░░░░░░░░▒░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▓███▓▓▓▓▒▒▒▒░▒▒▓▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓▓▓▓▓▒▒▒▓████▒░▒▒▒▒▓▒▒░▒░░░▒▒▒▒░▒░░░░░▓▓▓▒▒▒▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████▓█▓▓▓████▓▓▓▓▓▓▓▓▓▒▒▒▓▒▒▒▒▒▒▒▒▒▓▒▓▒▒▒▒▒▒▒▒▒▒▓▒░▒░▒▒▒▒▒░▒▒▒▒▒░░░░░░░░░░░░▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒▓██▓▓▓▒▒▒▒░░░░▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓██████▓▒▒▒▒▓▓▓▓▓▒▒▒▒░░▒▒▒▒▒▒▒▒▒░░▒▒▒▒░░░▒▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████▓▓▓▓███▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▒▒▒▒▒▒▒▒▓▒▓▓▒▒▓▒▓▓▒▒▒▒▒▒▒▒▓▓▒░░▒▒▓░░░▒░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒░░░▒▓███▓▓▒▒▒▒░░░░░░░░▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓████▓▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒░░░░░░░░▒▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▒▒▒▒▒▒▓▓▓▓▓▓▒▓▓▒█▓▓▒▓▒▒▒▒▓▒▒▒▒▒▓▒▒░░░░░▒▒▒░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒░░▒▒▓██▓▓▒▒▒▒▒▒▒░░░░░░▒▒▒▒▒▒▒▓▓▓▓▓▒▒▒▒▓█▓▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓██████████▓▒░░░░░░░▒▒▒░▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓████████████████████████████████████████████████████▓▓▓▓▒▓▓▓▓▓▒▓▓▓▓▒▒▒▒▒▒▓▓▓▓██▓▒▒▓▓█▓▓▓▓▓▓▓▓▓▒▒▒▓▓▒▓▒▒░▒▒▒▒▒▒▒▒░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒▓▓█▓▓▓▒▒▒▒▒▒▒░░░░░░▒░▒▒▒▒▒▒▒▒▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓█████████▓▒░░░░░░░░░▒▒▒▒▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓███▓▓▓▓▓▓█████████████████████████████████████████████████████▓▓▓▓▓▓▒▓▓▒▒▓▓▓▓▓▒▒▒▒▒▓███████▓█▓▓████▓█▓▓▓███▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒░▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒▓▓█▓▓▓▓▒▒▒▒▒▒▒░░░░░▒▒▒░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓███████▓▒░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓███████████████████████████████████████████████████▓██▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓██▓▓▓▓▓▓█▓▓██▓▓█▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▒▒▒▓▒▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒▓▓█▓▓▓▒▒▒▒▒▒▒▒▒▒▒░░▒░▒▒▒░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓█████▓░░░░░░░░░░░░░▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░▒▒▒▒▒▒░░░░░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▓████▓▒░░░░░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▒▓▓▓▒▓▓▓▓▓▓▒▒▒▓▒▓▒▓▒▒▒▓▓▓▓▓▒▒▒▓█▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░▒░▒▒░▒▒░░░░░░░░▒▒▒░▒░░▒▒▒▒▒▓███▒▒▒▒▒░░░░░░░░▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓▓▓███████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓█▓▓▒▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒▒▒▒░░░░░▒▒▒▒▒▒▒▒▒░░▒▒▒▓▓▒▒░░░▒▒▒▒░░░▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓▓▓██████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓█▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░▒░░░▒░▒▒▒░░░░░░▒▒▒▒▒░░░░░▒▒░▒▒▒░▒▒▒▒░░░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓▓████████████████████████████████████████████████████████████████████████████▓▓▓▓▓██▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░▒▒░▒░░░▒░▒▒▒▒▒▒▒░░░░▒░▒▒▒░░░░░░░░░░░░░░░░░░░░░▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓▓▓▓██████████████████████████████████████████████████████████████████████████████▓▓▓▓▓█▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░▒▒▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒░▒░░▒▒▒▒░░░░░▒▒▒▒▒▒▒▒░░░░░░▒▒▒░░░░░░░░░░░░░░░░░░░░░▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ▓▓██████████████████████████████████████████████████████████████████████████████████▓▓▓▓██▓▓▓▓▒▒▒▒▒▒▒▒▒░░▒░░░░░░░░░░░░░░░░░░░▒▒▒▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░▒▒▒▒░░▒▒▒▒▒▒░░░▒░▒▒░▒▒▒▒▒░▒▒▒▒▓▒▒▒▒▒▒░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒░▒░░░░░░░░░░░░░░░▒▒▒▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒░░░▒▒▒▒▒▒▒▒▒░░░░▒▒░▒▒▒▒▒▒▓▒▒▓▒▓▓▓▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████████████████████████████████████████████▓█▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒░░░░░▒▒▒▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░▒▒▒▒▒▒▒░▒░▒░░▒▒▒▒▒▒▒▒▓▓▒▓▓▓▓▓▓▓███████████████▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████████████████▓██████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒░░░░▒▒░▒▒▒░▒░░░▒░▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████▓███████████████████████████████████████████████████▓▓▓▓▓▓▒▓▓▓▓▓▓▒▒▒▒▓█▓▓▓▓██▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒░▒▒▒░░░▒▒▒░▒░▒░▒▒▒▒▒▒▒▓▒▒▓▓▓▓▓▓▓▓▓▓▓█▓▓▓█████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓███▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▒░░▒▒░▒░░░░░░░▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓█▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████▓███████████████████████████████████████████████████████████████████████████████████████████▓▓████▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒░░▒░▒▒▒░▒▒░░░░▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████▓████████████████████████████████████████████████████████████████████████████▓▓▓▓▓▓█████████▓▓▓████▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒░░░▒░▒▒▒░▒░░░▒░░░▒▒▒▒▒▒▒▒▒▒▓▒▒▒▒▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████▓████████████▓█▓████████████████████████████████████████████████████▓▒▓▓▓█▓▓▓▓▓█████████▓▓████▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒█▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒░▒░▒▒▒░▒▒░░▒▒░░░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓█▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████▓███████▓███████▓████████▓▓███████████████████████████████████████████████▓▒▒▒▒▒▓▓████▓▓██████████▓▓████▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▒▒▒▒▒▒▒▒▒▒░▒░░▒▒░▒░▒▒░░▒▒░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓█▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████▓███████████████▓▓████▓█▓▓▓▓██████████████████████████████████████▓▓▒▓▓▓▓▓▓▓▓▓█████▓▓██████████▓████▓▓▓▓███▓▒▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒░░▒░▒▒▒▒░░▒▒░░░▒░░▒░▒▒▒▒▒▒▒▒▒▒▒▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████▓█▓█▓▓██▓███▓▓███████▓▓▓▓▓███████▓▓▓███████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓██████████▓█████▓▓▓████▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▒▒▒▒▒▒▒▒░░▒▒░▒▒▒▒▒░░░▒░░▒░▒░▒▒▒▒▒▒▒▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████▓▓███▓▓▓▓▓▓▓▓▓█▓███████▓██▓▓███▓▓▓▓▓▓████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████▓██████▓▓███▓▓▒░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒░▒▒▒░░░░░▒░░░░░░▒░░▒░░░░░░▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓██▓▓▓▓▓▓█████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████▓▓▓████▓▒▒░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒░▒░░░░░░░▒░░░░░░░░░░░░░░░░▒▒▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓██████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████▓████████████▓▒░░░▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░░░░▒▒▒▒▒▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████▓▓▓█▓▓▓▓▓▓▓██▓▓▓▓▓▓██▓▓▓▓▓▓▓█▓███▓█▓█▓▓████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████▓█████████████▓▓▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░░░▒░░░░░▒▒▓▓█▓▓▓▓▓▓▒▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████▓██▓▓█▓█▓▓▓▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███▓▓▓▓██████████▓██▓▓████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████▓▓▓▓▒▒▒▒▒▒▒▒▒▒▒▓▓▓▓▓▒▓▓▓▓▓▒▒▒▒▒▒░░░░░░░░░░░░░░░░░░░▒▒▒▒▒█▓▓▓▓▓▓▓███▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████▓█▓▓██▓█▓██▓▓▓▓▓██▓▓██▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████▓▓▓▓▓▒▒░░▒▓▓▓▓██▓▓▓▓▓▓▓▒▓▒▓▓▓▓▓▒▒▒▒░░░░░░░░░░░░░░░▒▒▒█▓▓▓▓▓▓▓▓██▓█▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████▓▓▓██▓▓▓▓██████▓█▓███▓██████▓▓██▓▓█▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████▓▓▓▓▓▓▓███▓▓██▓▓▓▓█▓▓▓▒▒▓▓▓▒▒▒▓▒▒▓▒▓▒░░░░░░░░░▒▒▒▓███████████▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████▓▓█▓▓▓▓█▓█▓▓▓▓▓▓▓▓▓▓▓████████▓▓▓▓███▓▓▓▓▓▓▒▓████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████▓▓▓██████▓▓█▓▓▓▓▓█▓▓▓▒▒▓▓▓▒▓▒▒▒▓▒▒▒▒▒▒▓▒░░░░▒▒▒▓██████▓▓▓▓▓▓█████▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████▓████▓████████▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████▓▓▓▓▓▓▓▓▓▒▓██████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████▓██████▓██▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▒▒▓▓▓▓▒▒▒▒▒▓▒▒▒▒▒▒▒████▓▓██▓▓▓▓▒▒████▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████▓███▓██▓██████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓█▓▓▓▓▓▓▓▓▒▒▓█████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████▓▓██▓▓▓▓▓█▓▓▓▓▓█▓▓▒▒▓▓▓▓▒▓▒▓▓█▒▒▒▒▒▒▒▓███▓▓█▓▓▓▓▓▒▒▒████▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████▓██████▓█▓██▓▓▓█▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓█▓▓▓▓▓▓▓▒▓█▓████▓█▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████▓▓█▓▓▓▓▓█▓▓▓▒▓▓▓▓▓▓▒▓▓█▓▓▓▓▓▓▒▒▒▒▒▓▒▒▒▒▒▓▓▓▓▓▓▓▒▒▒▒▒███▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████▓▓██▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███▓██▓▓▓▓▓▒▓▓▓███████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████████▓▓██▓▓▒▓▓▓▓▓▓▓▓█▓▓▓▒▓▓▓▓▓▓▓▓▓▓▒▓▒▒▓▓▒▒▒▒▒▒▓▓▒▒▒▒▒▒▒▒▓██▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████▓█▓▓▓██▓█▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▒▒▒▓▓█▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████████▓▓█▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▒▒▓▓▒▒▒▓▒▒▒▒▒▒▒▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████▓██▓██▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓█▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████▓▓█▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▓▓▒▒▓▓▓▓▓▒▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████▓█▓▓▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▒▒▒▓▓▓█▓█▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████▓▓▓█▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▓▓▒▒▓▒▓▒▒▓▓▒▓▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████▓██▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▒▒▒▓▓▓▓██▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████▓██▓█▓▒█▓▓▓▓▒▓▓▒▓▓▒▓▒▓▓▒▓▒▓▓▓▒▒▒▓▓▒▒▒▓▓▓▓▓▓██▓█▓█▓██████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████▓█▓█▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▒▓▒▒▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████████████▓▓▓▓▓▒▒▒▓▓▓▓▒▒▓▓▓▒▒▒▓▓▒▒▒▓▒▓▓▓███████████▓███▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████▓▓██▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▒▓▓▓▓▒▒▒▓▒▒▒▒▓▒▓▓▓█████▓█████▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓███▓▓▒▒▒▓▒▓▒▓▓▒▒▒▓▒▒▓▓█████▓▓████▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▒▒▒▒▒▒▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓██▓▒▓▓▒▒▓▓▒▒▓▓▒▓▒▓█████▓▓██▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████▓██▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▒▓▒▒▒▓▓▓▓▓█▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████████████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▒▒▒▒▓███▓▓▒▒▓▒▒▓▒▒▓▓██████▓▓██▓▓▓▓█▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▒▒▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▓▒▒▓▓▓▓▓▓██▓▓▓▓▓▒▒▓▓██████▓▓██▓▓▓▓█▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████▓████▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████████▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▓▓▓▓▒▓▓███▓▓▒▓▓██████▓▓▓▓█▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████▓██▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▓▓▓▒▓▓▓▒▓▓▓▓▓▓▓▓▓▓▒▒▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▓▒▒▓▓▓▒▒▓██▓▓▓▓███████▓▓▓█▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▒▓▒▓▓▓▓▓▒▓▓▓▓▓▓▓▒▓▓▓▒▒▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓▒▒▒▓▓▓▓▒▒▒▓███▓▓███████▓▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓██████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▓▓▒▒▒▒▓▒▒▒▓▓▓█████▓█████▓▓▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▓▒▒▒▓▓▓▒▒▓▒▓████████████████▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓██████████████▓▓▓
                            ██████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓██████████████████████████████▓███████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▒▒▒▒▓▓▒▓▓▓▒▒▒▓▒▓▓█████████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████████▓███▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▓▓█████████████████████████████▓████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▒▒▒▒▒▒▒▓▓▓▒▒▓▒▓▓██████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▓▒▓▒▒▓▓▒▒▒▓▓███████████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████▓▓█▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████████████████████▓▓▓▓▓▓▓▒▒▓▒▒▒▒▒▒▓▒▒▒▓▓▓▒▓▓▓██████████▓▓█▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▒▓▒▒▒▒▒▒▒▒▒▒▓▓▓▒▒▓▓██████████████▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▓▓▓▒▓▓▓▒▒▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████████████▓▓▓▓▓▓▓▓▓███████████▓▓▓▓▓▓▓▒▒▒▓▒▒▓▒▒▓▓▒▓▓▒▓▓█████████████▓▓▓▓▓▓█▓█▓█▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓▓▒▒▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████▓▓▓▓▓▓▓▒▒▒▓▒▒▒▓▒▓▓▓▓▓▓▓██████████████▓█▓▓▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▒▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████▓▓▓▓▓▓▓▓▒▓▓▓▓▒▓▓▓▓▓▒▓██████████████▓█▓▓▓▓██▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓▓▓▓▒▒▒▓▒▓▒▒▒▓▓▓▒▓▓█████████████▓███▓▓▓█▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████▓████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓▒▓▒▓▓▓▓▓▓▓▓▓▓█████████████████▓████▓█▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████████▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓███████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓██████████████████▓███▓▓████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████▓█████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████▓▓▓▓▓▓▓██████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓███▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓███████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████████████████████▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            █████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓████▓▓▓▓▓██████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓███████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ███████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓██████▓▓▓████▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓███████▓▓▓▓███▓▓██▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ██████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            ████████████████████████████████████████████████████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█▓▓▓▓▓▓███████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▒▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓█████████████████████████▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓▓
                            Кто не понял, это Эйнштейн. И удивительно, но этот арт я не делал вручную*/