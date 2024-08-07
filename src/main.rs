mod checks;
mod crates;
mod godbolt;
mod helpers;
mod misc;
mod moderation;
mod playground;
mod types;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use types::{Context, Data};

use poise::serenity_prelude as serenity;

async fn on_error(error: poise::FrameworkError<'_, types::Data, Error>) {
    log::warn!("Encountered error: {:?}", error);
    if let poise::FrameworkError::ArgumentParse { error, ctx, .. } = error {
        let response = if error.is::<poise::CodeBlockError>() {
            "\
            Missing code block. Please use the following markdown:
            `` `code here` ``
            or
            ```ansi
            `\x1b[0m`\x1b[0m`rust
            code here
            `\x1b[0m`\x1b[0m`
            ```"
            .to_owned()
        } else if let Some(multiline_help) = &ctx.command().help_text {
            format!("**{}**\n{}", error, multiline_help)
        } else {
            error.to_string()
        };

        if let Err(e) = ctx.say(response).await {
            log::warn!("{}", e)
        }
    } else if let poise::FrameworkError::Command { ctx, error, .. } = error {
        if let Err(e) = ctx.say(error.to_string()).await {
            log::warn!("{}", e)
        }
    }
}

async fn on_pre_command(ctx: Context<'_>) {
    let channel_name =
        &ctx.channel_id().name(&ctx).await.unwrap_or_else(|_| "<unknown>".to_owned());
    let author = &ctx.author().name;

    log::info!(
        "{} in {} used slash command '{}'",
        author,
        channel_name,
        &ctx.invoked_command_name()
    );
}

fn env_var<T: std::str::FromStr>(name: &str) -> Result<T, Error>
where
    T::Err: std::fmt::Display,
{
    std::env::var(name)
        .map_err(|_| anyhow::anyhow!("Missing {}", name))?
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid {}: {}", name, e))
}

async fn app() -> Result<(), Error> {
    let discord_token = env_var::<String>("DISCORD_TOKEN")?;
    let mod_role_id = env_var("MOD_ROLE_ID")?;
    let reports_channel = env_var("REPORTS_CHANNEL_ID").ok();
    let discord_guild_id = env_var("DISCORD_SERVER_ID")?;

    let intents = serenity::GatewayIntents::non_privileged()
        | serenity::GatewayIntents::GUILD_MEMBERS
        | serenity::GatewayIntents::MESSAGE_CONTENT;

    let mut options = poise::FrameworkOptions {
        commands: vec![
            playground::play(),
            playground::playwarn(),
            playground::eval(),
            playground::miri(),
            playground::expand(),
            playground::clippy(),
            playground::fmt(),
            playground::microbench(),
            playground::procmacro(),
            godbolt::play_cpp(),
            godbolt::godbolt(),
            godbolt::mca(),
            godbolt::llvmir(),
            godbolt::asmdiff(),
            godbolt::targets_rust(),
            godbolt::targets_cpp(),
            crates::crate_(),
            crates::doc(),
            moderation::cleanup(),
            moderation::move_(),
            moderation::slowmode(),
            misc::source(),
            misc::help(),
            misc::register(),
            misc::uptime(),
            misc::servers(),
            misc::revision(),
            misc::conradluget(),
            misc::nicosay(),
        ],
        prefix_options: poise::PrefixFrameworkOptions {
            prefix: Some("?".into()),
            additional_prefixes: vec![
                poise::Prefix::Literal("cachyos_bot "),
                poise::Prefix::Literal("🦀 "),
                poise::Prefix::Literal("🦀"),
                poise::Prefix::Literal("<:ferris:358652670585733120> "),
                poise::Prefix::Literal("<:ferris:358652670585733120>"),
                poise::Prefix::Literal("<:ferrisballSweat:678714352450142239> "),
                poise::Prefix::Literal("<:ferrisballSweat:678714352450142239>"),
                poise::Prefix::Literal("<:ferrisCat:1183779700485664820> "),
                poise::Prefix::Literal("<:ferrisCat:1183779700485664820>"),
                poise::Prefix::Literal("<:ferrisOwO:579331467000283136> "),
                poise::Prefix::Literal("<:ferrisOwO:579331467000283136>"),
                poise::Prefix::Regex(
                    "(yo |hey )?(crab|ferris|fewwis),? can you (please |pwease )?".parse().unwrap(),
                ),
            ],
            edit_tracker: Some(Arc::new(poise::EditTracker::for_timespan(
                Duration::from_secs(60 * 5), // 5 minutes
            ))),
            ..Default::default()
        },
        // The global error handler for all error cases that may occur
        on_error: |error| Box::pin(on_error(error)),
        // This code is run before every command
        pre_command: |ctx| Box::pin(on_pre_command(ctx)),
        // This code is run after a command if it was successful (returned Ok)
        post_command: |ctx| {
            Box::pin(async move {
                log::info!("Executed command {}!", ctx.command().qualified_name);
            })
        },
        // Every command invocation must pass this check to continue execution
        command_check: Some(|_ctx| Box::pin(async move { Ok(true) })),
        // Enforce command checks even for owners (enforced by default)
        // Set to true to bypass checks, which is useful for testing
        skip_checks_for_owners: false,
        event_handler: |ctx, event, _framework, data| {
            Box::pin(async move { event_handler(ctx, event, data).await })
        },
        // Disallow all mentions (except those to the replied user) by default
        allowed_mentions: Some(serenity::CreateAllowedMentions::new().replied_user(true)),
        ..Default::default()
    };

    if reports_channel.is_some() {
        options.commands.push(moderation::report());
    }

    let framework =
        poise::Framework::builder()
            .setup(move |ctx, bot, _framework| {
                Box::pin(async move {
                    let data = Data {
                        bot_user_id: bot.user.id,
                        discord_guild_id,
                        mod_role_id,
                        reports_channel,
                        bot_start_time: std::time::Instant::now(),
                        http: reqwest::Client::new(),
                        godbolt_rust_targets: std::sync::Mutex::new(
                            godbolt::GodboltMetadata::default(),
                        ),
                        godbolt_cpp_targets: std::sync::Mutex::new(
                            godbolt::GodboltMetadata::default(),
                        ),
                        active_slowmodes: std::sync::Mutex::new(std::collections::HashMap::new()),
                    };

                    // log::debug!("Registering commands...");
                    // poise::builtins::register_in_guild(
                    // ctx,
                    // &framework.options().commands,
                    // data.discord_guild_id,
                    // )
                    // .await?;

                    log::debug!("Setting activity text");
                    ctx.set_activity(Some(serenity::ActivityData::listening("?help")));

                    Ok(data)
                })
            })
            .options(options)
            .build();

    serenity::ClientBuilder::new(discord_token, intents)
        .framework(framework)
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .start()
        .await?;

    Ok(())
}

async fn event_handler(
    _ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _data: &Data,
) -> Result<(), Error> {
    log::debug!("Got an event in event handler: {:?}", event.snake_case_name());

    Ok(())
}

#[tokio::main]
async fn main() {
    let _ = dotenv::dotenv();
    env_logger::init();

    if let Err(e) = app().await {
        log::error!("{}", e);
        std::process::exit(1);
    }
}
