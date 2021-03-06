use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "conversation_states")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub state_id: Uuid,
    pub parent: Uuid,
    #[sea_orm(column_type = "Text")]
    pub content: String,
    #[sea_orm(unique)]
    pub start_for: Option<Uuid>,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::conversations::Entity",
        from = "Column::Parent",
        to = "super::conversations::Column::ConversationId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    Conversations,
}

impl Related<super::conversations::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Conversations.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
