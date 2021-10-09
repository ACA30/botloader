use async_trait::async_trait;
use twilight_model::id::GuildId;

use crate::AnyError;

// A trait for handling errors in scripts mostly
// This might be moved into a more generalized "notification" trait later
//
// We need sync here because the provided method uses &self across an await
// (wich would normaly just make the future !Send but it fucks with the object safety somehow)
#[async_trait]
pub trait ErrorReporter: Sync {
    async fn report_script_error(
        &self,
        guild_id: GuildId,
        error: AnyError,
    ) -> Result<(), AnyError> {
        self.report_error(
            guild_id,
            format!(
                "An error occurred in one of your scripts:```\n{}\n```",
                error
            ),
        )
        .await
    }

    async fn report_error(&self, guild_id: GuildId, error: String) -> Result<(), AnyError>;
}

#[derive(Debug)]
pub struct NoOpErrorReporter;

#[async_trait]
impl ErrorReporter for NoOpErrorReporter {
    async fn report_error(&self, _: GuildId, _: String) -> Result<(), AnyError> {
        Ok(())
    }
}
