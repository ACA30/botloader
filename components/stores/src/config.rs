use std::error::Error;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use twilight_model::id::{ChannelId, GuildId, RoleId, UserId};

#[derive(Debug, Error)]
pub enum ConfigStoreError<T: std::fmt::Debug + Error + 'static> {
    #[error("script not found")]
    ScriptNotFound,

    #[error("script link not found")]
    LinkNotFound,

    #[error("inner error occured: {0}")]
    Other(#[from] T),
}

pub type StoreResult<T, U> = Result<T, ConfigStoreError<U>>;

#[async_trait]
pub trait ConfigStore: Clone + Sync {
    type Error: std::error::Error + Send + Sync;

    async fn get_script(
        &self,
        guild_id: GuildId,
        script_name: String,
    ) -> StoreResult<Script, Self::Error>;
    async fn create_script(
        &self,
        guild_id: GuildId,
        script: CreateScript,
    ) -> StoreResult<Script, Self::Error>;
    async fn update_script(
        &self,
        guild_id: GuildId,
        script: Script,
    ) -> StoreResult<Script, Self::Error>;
    async fn del_script(
        &self,
        guild_id: GuildId,
        script_name: String,
    ) -> StoreResult<(), Self::Error>;
    async fn list_scripts(&self, guild_id: GuildId) -> StoreResult<Vec<Script>, Self::Error>;

    async fn link_script(
        &self,
        guild_id: GuildId,
        script_name: String,
        ctx: ScriptContext,
    ) -> StoreResult<ScriptLink, Self::Error>;

    async fn unlink_script(
        &self,
        guild_id: GuildId,
        script_name: String,
        ctx: ScriptContext,
    ) -> StoreResult<(), Self::Error>;

    async fn unlink_all_script(
        &self,
        guild_id: GuildId,
        script_name: String,
    ) -> StoreResult<u64, Self::Error>;

    async fn list_script_links(
        &self,
        guild_id: GuildId,
        script_name: String,
    ) -> StoreResult<Vec<ScriptLink>, Self::Error>;

    async fn list_links(&self, guild_id: GuildId) -> StoreResult<Vec<ScriptLink>, Self::Error>;

    async fn list_context_scripts(
        &self,
        guild_id: GuildId,
        ctx: ScriptContext,
    ) -> StoreResult<Vec<Script>, Self::Error>;

    async fn get_guild_meta_config(
        &self,
        guild_id: GuildId,
    ) -> StoreResult<Option<GuildMetaConfig>, Self::Error>;
    async fn update_guild_meta_config(
        &self,
        conf: &GuildMetaConfig,
    ) -> StoreResult<GuildMetaConfig, Self::Error>;

    async fn get_guild_meta_config_or_default(
        &self,
        guild_id: GuildId,
    ) -> StoreResult<GuildMetaConfig, Self::Error> {
        match self.get_guild_meta_config(guild_id).await {
            Ok(Some(conf)) => Ok(conf),
            Ok(None) => Ok(GuildMetaConfig {
                guild_id,
                ..Default::default()
            }),
            Err(e) => Err(e),
        }
    }

    async fn add_update_joined_guild(
        &self,
        guild: JoinedGuild,
    ) -> StoreResult<JoinedGuild, Self::Error>;

    async fn remove_joined_guild(&self, guild_id: GuildId) -> StoreResult<bool, Self::Error>;

    async fn get_joined_guilds(
        &self,
        ids: &[GuildId],
    ) -> StoreResult<Vec<JoinedGuild>, Self::Error>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub id: u64,
    pub name: String,
    pub original_source: String,
    pub compiled_js: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScript {
    pub name: String,
    pub original_source: String,
    pub compiled_js: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ScriptLink {
    pub script_name: String,
    pub context: ScriptContext,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ScriptContext {
    Guild,
    Channel(ChannelId),
    Role(RoleId),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct GuildMetaConfig {
    pub guild_id: GuildId,
    pub error_channel_id: Option<ChannelId>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JoinedGuild {
    pub id: GuildId,
    pub name: String,
    pub icon: String,
    pub owner_id: UserId,
}
