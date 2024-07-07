use anyhow::Error;
use poise::serenity_prelude::EditChannel;

use crate::types::Context;

async fn immediately_lift_slowmode(ctx: Context<'_>) -> anyhow::Result<()> {
    let active_slowmode = ctx.data().active_slowmodes.lock().unwrap().remove(&ctx.channel_id());

    match active_slowmode {
        Some(active_slowmode) => {
            let builder = EditChannel::new()
                .rate_limit_per_user(active_slowmode.previous_slowmode_rate.try_into().unwrap());
            ctx.channel_id().edit(&ctx, builder).await?;
            ctx.say("Restored slowmode to previous level").await?;
        },
        None => {
            ctx.say("There is no slowmode command currently running").await?;
        },
    }

    Ok(())
}

async fn register_slowmode(
    ctx: Context<'_>,
    duration_argument: Option<u64>,
    rate_argument: Option<u64>,
) -> Result<(u64, u64), Error> {
    let current_slowmode_rate = match ctx.channel_id().to_channel(&ctx).await {
        Ok(channel) => channel
            .guild()
            .ok_or(anyhow::anyhow!("This command only works inside guilds"))?
            .rate_limit_per_user
            .unwrap_or(0),
        Err(e) => {
            log::warn!("Couldn't retrieve channel slowmode settings: {}", e);
            0
        },
    };

    let mut active_slowmodes = ctx.data().active_slowmodes.lock().unwrap();
    let already_active_slowmode = active_slowmodes.get(&ctx.channel_id());

    // If we're overwriting an existing slowmode command, the channel's current slowmode rate
    // is not the original one, so we check the existing entry
    let previous_slowmode_rate = already_active_slowmode
        .map_or(current_slowmode_rate, |s| s.previous_slowmode_rate.try_into().unwrap());
    let duration =
        duration_argument.or_else(|| Some(already_active_slowmode?.duration)).unwrap_or(30);
    let rate = rate_argument.or_else(|| Some(already_active_slowmode?.rate)).unwrap_or(15);

    active_slowmodes.insert(ctx.channel_id(), crate::types::ActiveSlowmode {
        previous_slowmode_rate: previous_slowmode_rate.into(),
        duration,
        rate,
        invocation_time: *ctx.created_at(),
    });

    Ok((duration, rate))
}

async fn restore_slowmode_rate(ctx: Context<'_>) -> Result<(), Error> {
    let previous_slowmode_rate = {
        let mut active_slowmodes = ctx.data().active_slowmodes.lock().unwrap();
        let active_slowmode = match active_slowmodes.remove(&ctx.channel_id()) {
            Some(x) => x,
            None => {
                log::info!(
                    "Slowmode entry has expired; this slowmode invocation has been overwritten"
                );
                return Ok(());
            },
        };
        if active_slowmode.invocation_time != *ctx.created_at() {
            log::info!(
                "Slowmode entry has a different invocation time; this slowmode invocation has \
                 been overwritten"
            );
            return Ok(());
        }
        active_slowmode.previous_slowmode_rate
    };

    log::info!("Restoring slowmode rate to {}", previous_slowmode_rate);

    let builder =
        EditChannel::new().rate_limit_per_user(previous_slowmode_rate.try_into().unwrap());
    ctx.channel_id().edit(&ctx, builder).await?;
    ctx.data().active_slowmodes.lock().unwrap().remove(&ctx.channel_id());

    Ok(())
}

/// Temporarily enables slowmode for this channel (moderator only)
///
/// After the specified duration, the slowmode will be reset to previous level. Invoke the command \
/// with duration set to zero to immediately lift slowmode. If the command is invoked while an
/// existing invocation is running, the running invocation will be overwritten.
///
/// Default duration: 30 minutes
/// Default rate: 15 seconds
#[poise::command(slash_command, prefix_command, hide_in_help, category = "Moderation")]
pub async fn slowmode(
    ctx: Context<'_>,
    #[description = "How long slowmode should persist for this channel, in minutes"]
    duration: Option<u64>, // TODO: make f32 with a #[min = 0.0] attribute (once poise supports it)
    #[description = "How many seconds a user has to wait before sending another message (0-120)"]
    rate: Option<u64>,
) -> Result<(), Error> {
    if !crate::checks::check_is_moderator(ctx).await? {
        return Ok(());
    }

    if duration == Some(0) || rate == Some(0) {
        immediately_lift_slowmode(ctx).await?;
        return Ok(());
    }

    // Register that there is an active slowmode command, or overwrite an existing entry.
    // In the end, we can make sure that our slowmode command invocation has not been overwritten
    // since by a new invocation
    let (duration, rate) = register_slowmode(ctx, duration, rate).await?;

    // Apply slowmode
    let builder = EditChannel::new().rate_limit_per_user(rate.try_into().unwrap());
    ctx.channel_id().edit(&ctx, builder).await?;

    // Confirmation message
    let _: Result<_, _> = ctx
        .say(format!(
            "Slowmode will be enabled for {} minutes. Members can send one message every {} \
             seconds",
            duration, rate,
        ))
        .await;

    // Wait until slowmode is over
    tokio::time::sleep(std::time::Duration::from_secs(60 * duration)).await;

    restore_slowmode_rate(ctx).await?;

    Ok(())
}
