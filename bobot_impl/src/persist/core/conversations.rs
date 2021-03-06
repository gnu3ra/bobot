use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "conversations")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub conversation_id: Uuid,
    #[sea_orm(column_type = "Text")]
    pub triggerphrase: String,
    pub chat_id: Option<i64>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(has_many = "super::conversation_states::Entity")]
    ConversationStates,
}

impl Related<super::conversation_states::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::ConversationStates.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
