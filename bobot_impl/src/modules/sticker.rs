use std::str::FromStr;

use self::entities::tags::ModelRedis;
use crate::persist::redis::{
    default_cached_query_vec, scope_key_by_chatuser, CachedQuery, CachedQueryTrait, RedisPool,
    RedisStr,
};
use crate::persist::Result;
use crate::statics::{DB, REDIS, TG};
use crate::tg::command::{parse_cmd, Arg};
use crate::tg::dialog::{drop_converstaion, Conversation};
use crate::tg::dialog::{get_conversation, replace_conversation};
use crate::util::error::BotError;
use anyhow::anyhow;
use lazy_static::__Deref;
use log::info;
use sea_orm::entity::prelude::*;
use sea_orm::{ActiveModelTrait, IntoActiveModel, QuerySelect, Set};
use sea_schema::migration::{MigrationName, MigrationTrait};

use teloxide::payloads::SendMessageSetters;
use teloxide::prelude::Requester;
use teloxide::types::{
    InlineQuery, InlineQueryResult, InlineQueryResultCachedSticker, MediaKind, Message,
    MessageCommon, MessageKind, Update, UpdateKind,
};

// redis keys
const KEY_TYPE_TAG: &str = "wc:tag";
const KEY_TYPE_STICKER_ID: &str = "wc:stickerid";
const KEY_TYPE_STICKER_NAME: &str = "wc:stickername";

// conversation state machine globals
const UPLOAD_CMD: &str = "/upload";
const TRANSITION_NAME: &str = "stickername";
const TRANSITION_DONE: &str = "stickerdone";
const TRANSITION_UPLOAD: &str = "upload";
const TRANSITION_TAG: &str = "stickertag";
const TRANSITION_MORETAG: &str = "stickermoretag";
const STATE_START: &str = "Send a sticker to upload";
const STATE_UPLOAD: &str = "sticker uploaded";
const STATE_NAME: &str = "Send a name for this sticker";
const STATE_TAGS: &str = "Send tags for this sticker, one at a time. Send /done to stop";
const STATE_DONE: &str = "Successfully uploaded sticker";

fn upload_sticker_conversation(message: &Message) -> Result<Conversation> {
    let mut conversation = Conversation::new(
        UPLOAD_CMD.to_string(),
        STATE_START.to_string(),
        message.chat.id,
        message
            .from()
            .ok_or_else(|| BotError::new("message has no sender"))?
            .id,
    )?;
    let start_state = conversation.get_start()?.state_id;
    let upload_state = conversation.add_state(STATE_UPLOAD);
    let name_state = conversation.add_state(STATE_NAME);
    let state_tags = conversation.add_state(STATE_TAGS);
    let state_done = conversation.add_state(STATE_DONE);

    conversation.add_transition(start_state, upload_state, TRANSITION_UPLOAD);
    conversation.add_transition(upload_state, name_state, TRANSITION_NAME);
    conversation.add_transition(name_state, state_tags, TRANSITION_TAG);
    conversation.add_transition(state_tags, state_tags, TRANSITION_MORETAG);
    conversation.add_transition(state_tags, state_done, TRANSITION_DONE);

    Ok(conversation)
}

struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m20220412_000001_create_stickertag"
    }
}

pub mod entities {
    use crate::persist::migrate::ManagerHelper;
    use sea_schema::migration::prelude::*;
    #[async_trait::async_trait]
    impl MigrationTrait for super::Migration {
        async fn up(
            &self,
            manager: &sea_schema::migration::SchemaManager,
        ) -> std::result::Result<(), sea_orm::DbErr> {
            manager
                .create_table(
                    Table::create()
                        .table(tags::Entity)
                        .col(
                            ColumnDef::new(tags::Column::Id)
                                .big_integer()
                                .primary_key()
                                .auto_increment(),
                        )
                        .col(ColumnDef::new(tags::Column::StickerId).text().not_null())
                        .col(
                            ColumnDef::new(tags::Column::OwnerId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(tags::Column::Tag).text().not_null())
                        .to_owned(),
                )
                .await?;

            manager
                .create_table(
                    Table::create()
                        .table(stickers::Entity)
                        .col(
                            ColumnDef::new(stickers::Column::UniqueId)
                                .text()
                                .primary_key(),
                        )
                        .col(ColumnDef::new(stickers::Column::Uuid).uuid().unique_key())
                        .col(
                            ColumnDef::new(stickers::Column::OwnerId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(stickers::Column::ChosenName).text())
                        .to_owned(),
                )
                .await?;

            manager
                .create_foreign_key(
                    ForeignKey::create()
                        .from(tags::Entity, tags::Column::StickerId)
                        .to(stickers::Entity, stickers::Column::UniqueId)
                        .on_delete(ForeignKeyAction::Cascade)
                        .to_owned(),
                )
                .await?;

            Ok(())
        }

        async fn down(
            &self,
            manager: &sea_schema::migration::SchemaManager,
        ) -> std::result::Result<(), sea_orm::DbErr> {
            manager.drop_table_auto(tags::Entity).await?;
            manager.drop_table_auto(stickers::Entity).await?;
            Ok(())
        }
    }
    pub mod tags {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "tags")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = true)]
            pub id: i64,
            pub sticker_id: String,
            pub owner_id: i64,
            #[sea_orm(column_type = "Text")]
            pub tag: String,
        }

        #[derive(DeriveIntoActiveModel, Serialize, Deserialize)]
        pub struct ModelRedis {
            pub sticker_id: String,
            pub owner_id: i64,
            pub tag: String,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(
                belongs_to = "super::stickers::Entity",
                from = "Column::StickerId",
                to = "super::stickers::Column::UniqueId"
            )]
            Stickers,
        }

        impl Related<super::stickers::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Stickers.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }

    pub mod stickers {
        use sea_orm::entity::prelude::*;
        use serde::{Deserialize, Serialize};

        #[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
        #[sea_orm(table_name = "stickers")]
        pub struct Model {
            #[sea_orm(primary_key, auto_increment = false)]
            pub unique_id: String,
            pub owner_id: i64,
            #[sea_orm(unique)]
            pub uuid: Uuid,
            #[sea_orm(column_type = "Text", nullable)]
            pub chosen_name: Option<String>,
        }

        #[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
        pub enum Relation {
            #[sea_orm(has_many = "super::tags::Entity")]
            Tags,
        }

        impl Related<super::tags::Entity> for Entity {
            fn to() -> RelationDef {
                Relation::Tags.def()
            }
        }

        impl ActiveModelBehavior for ActiveModel {}
    }
}

pub fn get_migrations() -> Vec<Box<dyn MigrationTrait>> {
    vec![Box::new(Migration)]
}

async fn handle_inline(query: &InlineQuery) -> Result<()> {
    log::info!("query! owner: {} tag: {}", query.from.id, query.query);
    let id = query.from.id;
    let key = query.query.to_owned();
    if let Some(stickers) = tokio::spawn(async move {
        default_cached_query_vec(move |key, sql| async move {
            let sql: &DatabaseConnection = sql;
            let key = format!("%{}%", key);
            let stickers = entities::stickers::Entity::find()
                .join(
                    sea_orm::JoinType::InnerJoin,
                    entities::stickers::Relation::Tags.def(),
                )
                .group_by(entities::stickers::Column::UniqueId)
                .filter(entities::stickers::Column::OwnerId.eq(id))
                .filter(entities::tags::Column::Tag.like(&key))
                .limit(10)
                .all(sql)
                .await?;
            Ok(Some(stickers))
        })
        .query(&DB.deref(), &REDIS, &key)
        .await
    })
    .await??
    {
        let stickers = stickers.into_iter().map(|s| {
            let r = InlineQueryResultCachedSticker {
                id: Uuid::new_v4().to_string(),
                sticker_file_id: s.unique_id,
                reply_markup: None,
                input_message_content: None,
            };
            InlineQueryResult::CachedSticker(r)
        });

        TG.client
            .answer_inline_query(query.id.as_str(), stickers)
            .await?;
    }
    Ok(())
}

async fn handle_message(message: &Message) -> Result<()> {
    handle_command(message).await?;
    handle_conversation(message).await?;
    Ok(())
}

pub async fn handle_update(update: &Update) {
    let res = match update.kind {
        UpdateKind::Message(ref message) => handle_message(message).await,
        UpdateKind::InlineQuery(ref query) => handle_inline(query).await,
        _ => Ok(()),
    };
    if let Err(err) = res {
        info!("error {}", err);
        if let Some(chat) = update.chat() {
            if let Err(send_err) = TG.client().send_message(chat.id, err.to_string()).await {
                log::error!("failed to send error message: {}", send_err);
            }
        }
    }
}

async fn handle_command(message: &Message) -> Result<()> {
    if let Some(text) = message.text() {
        let command = parse_cmd(text)?;
        if let Some(Arg::Arg(cmd)) = command.first() {
            info!("command {}", cmd);
            match cmd.as_str() {
                "/upload" => upload(message).await,
                "/list" => list_stickers(message).await,
                "/delete" => delete_sticker(message, command).await,
                _ => Ok(()),
            }?;
        }
    };

    Ok(())
}

async fn upload(message: &Message) -> Result<()> {
    replace_conversation(message, |message| upload_sticker_conversation(message)).await?;
    Ok(())
}

async fn delete_sticker(message: &Message, args: Vec<Arg>) -> Result<()> {
    drop_converstaion(message).await?;
    if let [Arg::Arg(_), Arg::Arg(uuid)] = args.as_slice() {
        let uuid = Uuid::from_str(uuid.as_str())?;
        entities::stickers::Entity::delete_many()
            .filter(entities::stickers::Column::Uuid.eq(uuid))
            .exec(DB.deref().deref())
            .await?;
        TG.client()
            .send_message(message.chat.id, "Successfully deleted sticker")
            .reply_to_message_id(message.id)
            .await?;
        Ok(())
    } else {
        Err(anyhow!(BotError::new("invalid command args")))
    }
}

async fn list_stickers(message: &Message) -> Result<()> {
    drop_converstaion(message).await?;
    if let Some(sender) = message.from() {
        let stickers = entities::stickers::Entity::find()
            .filter(entities::stickers::Column::OwnerId.eq(sender.id))
            .all(DB.deref().deref())
            .await?;
        let stickers = stickers
            .into_iter()
            .fold(String::from("My stickers:"), |mut s, sticker| {
                let default = "Unnamed".to_string();
                let chosenname = sticker.chosen_name.as_ref().unwrap_or(&default);
                s.push_str(format!("\n - {} {}", chosenname, sticker.uuid).as_str());
                s
            });

        TG.client()
            .send_message(message.chat.id, stickers)
            .reply_to_message_id(message.id)
            .await?;
    }
    Ok(())
}

async fn conv_start(conversation: Conversation, message: &Message) -> Result<()> {
    TG.client()
        .send_message(message.chat.id, "Send a sticker to upload")
        .reply_to_message_id(message.id)
        .await?;
    conversation.transition(TRANSITION_UPLOAD).await?;
    Ok(())
}

async fn conv_upload(conversation: Conversation, message: &Message) -> Result<()> {
    if let MessageKind::Common(MessageCommon {
        media_kind: MediaKind::Sticker(ref sticker),
        ..
    }) = message.kind
    {
        let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
        let taglist = scope_key_by_chatuser(&KEY_TYPE_TAG, &message)?;
        REDIS
            .pipe(|p| {
                p.set(&key, &sticker.sticker.file_id);
                p.del(&taglist)
            })
            .await?;
        let text = conversation.transition(TRANSITION_NAME).await?;
        TG.client()
            .send_message(message.chat.id, text)
            .reply_to_message_id(message.id)
            .await?;
        Ok(())
    } else {
        Err(anyhow!(BotError::new("Send a sticker")))
    }
}

async fn conv_name(conversation: Conversation, message: &Message) -> Result<()> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    REDIS.pipe(|p| p.set(&key, message.text())).await?;
    let text = conversation.transition(TRANSITION_TAG).await?;
    TG.client()
        .send_message(message.chat.id, text)
        .reply_to_message_id(message.id)
        .await?;
    Ok(())
}

async fn conv_moretags(conversation: Conversation, message: &Message) -> Result<()> {
    let key = scope_key_by_chatuser(&KEY_TYPE_STICKER_ID, &message)?;
    let namekey = scope_key_by_chatuser(&KEY_TYPE_STICKER_NAME, &message)?;
    let taglist = scope_key_by_chatuser(&KEY_TYPE_TAG, &message)?;

    let sticker_id: (String,) = REDIS.pipe(|p| p.get(&key)).await?;
    let sticker_id = sticker_id.0;
    let text = message.text().ok_or_else(|| BotError::new("no text"))?;
    info!("moretags stickerid: {}", sticker_id);
    if let Some(user) = message.from() {
        if text == "/done" {
            let stickername: (String,) = REDIS.pipe(|p| p.get(&namekey)).await?;
            let stickername = stickername.0;

            let tags = REDIS
                .drain_list::<ModelRedis>(&taglist)
                .await?
                .into_iter()
                .map(|m| {
                    info!("tag id {}", m.sticker_id);
                    m.into_active_model()
                });

            info!("inserting sticker {}", sticker_id);

            let sticker = entities::stickers::ActiveModel {
                unique_id: Set(sticker_id),
                owner_id: Set(user.id),
                uuid: Set(Uuid::new_v4()),
                chosen_name: Set(Some(stickername)),
            };

            sticker.insert(DB.deref().deref()).await?;

            info!("inserting tags {}", tags.len());
            entities::tags::Entity::insert_many(tags)
                .exec(DB.deref().deref())
                .await?;

            let text = conversation.transition(TRANSITION_DONE).await?;
            TG.client()
                .send_message(message.chat.id, text)
                .reply_to_message_id(message.id)
                .await?;
            Ok(())
        } else {
            let tag = RedisStr::new(&ModelRedis {
                sticker_id,
                owner_id: user.id,
                tag: text.to_owned(),
            })?;

            REDIS
                .pipe(|p| {
                    p.atomic();
                    p.lpush(taglist, &tag)
                })
                .await?;

            let text = conversation.transition(TRANSITION_MORETAG).await?;
            TG.client()
                .send_message(message.chat.id, text)
                .reply_to_message_id(message.id)
                .await?;
            Ok(())
        }
    } else {
        Err(anyhow!(BotError::new("not a user")))
    }
}

async fn handle_conversation(message: &Message) -> Result<()> {
    if let Some(conversation) = get_conversation(&message).await? {
        match conversation.get_current_text().await?.as_str() {
            STATE_START => conv_start(conversation, &message).await,
            STATE_UPLOAD => conv_upload(conversation, &message).await,
            STATE_NAME => conv_name(conversation, &message).await,
            STATE_TAGS => conv_moretags(conversation, &message).await,
            _ => return Ok(()),
        }?;
    } else {
        info!("nope no conversation for u");
    }
    Ok(())
}
