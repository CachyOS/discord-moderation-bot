use anyhow::Error;
use poise::serenity_prelude as serenity;

use crate::godbolt;

#[derive(Clone, Debug)]
pub struct ActiveSlowmode {
    pub previous_slowmode_rate: u64,
    pub duration: u64,
    pub rate: u64,
    /// The slowmode command verifies this value after the sleep and before the slowdown lift,
    /// to make sure that no new slowmode command has been invoked since
    pub invocation_time: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug)]
pub struct Data {
    pub bot_user_id: serenity::UserId,
    pub discord_guild_id: serenity::GuildId,
    #[allow(dead_code)] // might add back in
    pub mod_role_id: serenity::RoleId,
    pub reports_channel: Option<serenity::ChannelId>,
    pub bot_start_time: std::time::Instant,
    pub http: reqwest::Client,
    pub database: sqlx::SqlitePool,
    pub godbolt_rust_targets: std::sync::Mutex<godbolt::GodboltMetadata>,
    pub godbolt_cpp_targets: std::sync::Mutex<godbolt::GodboltMetadata>,
    pub active_slowmodes:
        std::sync::Mutex<std::collections::HashMap<serenity::ChannelId, ActiveSlowmode>>,
}

pub type Context<'a> = poise::Context<'a, Data, Error>;

// const EMBED_COLOR: (u8, u8, u8) = (0xf7, 0x4c, 0x00);
pub const EMBED_COLOR: (u8, u8, u8) = (0xb7, 0x47, 0x00); // slightly less saturated
