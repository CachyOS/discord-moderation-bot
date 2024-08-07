mod slowmode;
pub use slowmode::slowmode;

use anyhow::Error;
use poise::serenity_prelude::{
    self as serenity, CreateAllowedMentions, CreateMessage, CreateThread, GetMessages,
};

use crate::types::Context;

/// Deletes the bot's messages for cleanup
///
/// /cleanup [limit]
///
/// By default, only the most recent bot message is deleted (limit = 1).
///
/// Deletes the bot's messages for cleanup.
/// You can specify how many messages to look for. Only the 20 most recent messages within the
/// channel from the last 24 hours can be deleted.
#[poise::command(
    prefix_command,
    slash_command,
    category = "Moderation",
    on_error = "crate::helpers::acknowledge_fail"
)]
pub async fn cleanup(
    ctx: Context<'_>,
    #[description = "Number of messages to delete"] num_messages: Option<usize>,
) -> Result<(), Error> {
    let num_messages = num_messages.unwrap_or(1);

    let messages_to_delete = ctx
        .channel_id()
        .messages(&ctx, serenity::GetMessages::new().limit(50))
        .await?
        .into_iter()
        .filter(|msg| {
            (msg.author.id == ctx.data().bot_user_id)
                && (*ctx.created_at() - *msg.timestamp).num_hours() < 24
        })
        .take(num_messages);

    ctx.channel_id().delete_messages(&ctx, messages_to_delete).await?;

    crate::helpers::acknowledge_success(ctx, "cat_uwu", '👌').await
}

async fn latest_message_link(ctx: Context<'_>) -> String {
    let builder = GetMessages::new().limit(1);
    let message = ctx
        .channel_id()
        .messages(&ctx, builder)
        .await
        .ok()
        .and_then(|messages| messages.into_iter().next());
    match message {
        Some(msg) => msg.link_ensured(&ctx).await,
        None => "<couldn't retrieve latest message link>".into(),
    }
}

/// Discreetly reports a user for breaking the rules
///
/// Call this command in a channel when someone might be breaking the rules, for example by being \
/// very rude, or starting discussions about divisive topics like politics and religion. Nobody \
/// will see that you invoked this command.
///
/// Your report, along with a link to the \
/// channel and its most recent message, will show up in a dedicated reports channel for \
/// moderators, and it allows them to deal with it much faster than if you were to DM a \
/// potentially AFK moderator.
///
/// You can still always ping the Moderator role if you're comfortable doing so.
#[poise::command(slash_command, ephemeral, hide_in_help, category = "Moderation")]
pub async fn report(
    ctx: Context<'_>,
    #[description = "What did the user do wrong?"] reason: String,
) -> anyhow::Result<()> {
    let reports_channel =
        ctx.data().reports_channel.ok_or(anyhow::anyhow!("No reports channel was configured"))?;

    let naughty_channel = ctx
        .channel_id()
        .to_channel(&ctx)
        .await?
        .guild()
        .ok_or(anyhow::anyhow!("This command can only be used in a guild"))?;

    let report_name = format!("Report {}", ctx.id() % 1000);

    let builder = CreateThread::new(report_name).kind(serenity::ChannelType::PrivateThread);

    // let msg = reports_channel.say(&ctx, &report_name).await?;
    let report_thread = reports_channel.create_thread(&ctx, builder).await?;

    let thread_message_content = format!(
        "Hey <@&{}>, <@{}> sent a report from channel {}: {}\n> {}",
        ctx.data().mod_role_id.get(),
        ctx.author().id.get(),
        naughty_channel.name,
        latest_message_link(ctx).await,
        reason
    );

    let allowed_mentions =
        CreateAllowedMentions::new().users([ctx.author().id]).roles([ctx.data().mod_role_id]);
    let builder =
        CreateMessage::new().content(thread_message_content).allowed_mentions(allowed_mentions);
    report_thread.send_message(&ctx, builder).await?;

    ctx.say("Successfully sent report. Thanks for helping to make this community a better place!")
        .await?;

    Ok(())
}

/// Move a discussion to another channel
///
/// Move a discussion to a specified channel, optionally pinging a list of users in the new channel.
#[poise::command(prefix_command, rename = "move", aliases("migrate"), category = "Moderation")]
pub async fn move_(
    ctx: Context<'_>,
    #[description = "Where to move the discussion"] target_channel: serenity::GuildChannel,
    #[description = "Participants of the discussion who will be pinged in the new channel"]
    users_to_ping: Vec<serenity::Member>,
) -> anyhow::Result<()> {
    use serenity::Mentionable as _;

    if Some(target_channel.guild_id) != ctx.guild_id() {
        anyhow::bail!("Can't move discussion across servers");
    }

    // DON'T use GuildChannel::permissions_for_user - it requires member to be cached
    let guild = ctx.data().discord_guild_id.to_partial_guild(&ctx).await?;
    let member = guild.member(&ctx, ctx.author().id).await?;
    let permissions_in_target_channel = guild.user_permissions_in(&target_channel, &member);
    if !permissions_in_target_channel.send_messages() {
        anyhow::bail!("You don't have permission to post in {}", target_channel.mention());
    }

    let source_msg_link = match ctx {
        Context::Prefix(ctx) => ctx.msg.link_ensured(&ctx).await,
        _ => latest_message_link(ctx).await,
    };

    let mut comefrom_message = format!(
        "**Discussion moved here from {}**\n{}",
        ctx.channel_id().mention(),
        source_msg_link
    );

    {
        let mut users_to_ping = users_to_ping.iter();
        if let Some(user_to_ping) = users_to_ping.next() {
            comefrom_message += &format!("\n{}", user_to_ping.mention());
            for user_to_ping in users_to_ping {
                comefrom_message += &format!(", {}", user_to_ping.mention());
            }
        }
    }

    // let comefrom_message = target_channel.say(&ctx, comefrom_message).await?;
    let allowed_mentions = CreateAllowedMentions::new().users(users_to_ping);
    let builder = CreateMessage::new().content(comefrom_message).allowed_mentions(allowed_mentions);
    let comefrom_message = target_channel.send_message(&ctx, builder).await?;

    ctx.say(format!(
        "**{} suggested to move this discussion to {}**\n{}",
        &ctx.author().tag(),
        target_channel.mention(),
        comefrom_message.link_ensured(&ctx).await
    ))
    .await?;

    Ok(())
}
