use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "conversation_transitions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub transition_id: Uuid,
    pub start_state: Uuid,
    pub end_state: Uuid,
    #[sea_orm(column_type = "Text")]
    pub triggerphrase: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::conversation_states::Entity",
        from = "Column::EndState",
        to = "super::conversation_states::Column::StateId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    ConversationStates2,
    #[sea_orm(
        belongs_to = "super::conversation_states::Entity",
        from = "Column::StartState",
        to = "super::conversation_states::Column::StateId",
        on_update = "NoAction",
        on_delete = "Cascade"
    )]
    ConversationStates1,
}

impl ActiveModelBehavior for ActiveModel {}
