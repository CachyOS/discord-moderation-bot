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
    Ok(std::env::var(name)
        .map_err(|_| anyhow::anyhow!("Missing {}", name))?
        .parse()
        .map_err(|e| anyhow::anyhow!("Invalid {}: {}", name, e))?)
}

async fn app() -> Result<(), Error> {
    let discord_token = env_var::<String>("DISCORD_TOKEN")?;
    let mod_role_id = env_var("MOD_ROLE_ID")?;
    let reports_channel = env_var("REPORTS_CHANNEL_ID").ok();
    let database_url = env_var::<String>("DATABASE_URL")?;
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

    let database = sqlx::sqlite::SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(
            database_url.parse::<sqlx::sqlite::SqliteConnectOptions>()?.create_if_missing(true),
        )
        .await?;
    sqlx::migrate!("./migrations").run(&database).await?;

    let framework =
        poise::Framework::builder()
            .setup(move |ctx, bot, framework| {
                Box::pin(async move {
                    let data = Data {
                        bot_user_id: bot.user.id,
                        discord_guild_id,
                        mod_role_id,
                        reports_channel,
                        bot_start_time: std::time::Instant::now(),
                        http: reqwest::Client::new(),
                        database,
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

    let _ = serenity::ClientBuilder::new(discord_token, intents)
        .framework(framework)
        .await
        .map_err(|e| anyhow::anyhow!(e))?
        .start()
        .await?;

    Ok(())
}

/// Truncates the message with a given truncation message if the
/// text is too long. "Too long" means, it either goes beyond Discord's 2000 char message limit,
/// or if the text_body has too many lines.
///
/// Only `text_body` is truncated. `text_end` will always be appended at the end. This is useful
/// for example for large code blocks. You will want to truncate the code block contents, but the
/// finalizing triple backticks (` ` `) should always stay - that's what `text_end` is for.
async fn trim_text(
    mut text_body: &str,
    text_end: &str,
    truncation_msg_future: impl std::future::Future<Output = String>,
) -> String {
    const MAX_OUTPUT_LINES: usize = 45;

    // Err with the future inside if no truncation occurs
    let mut truncation_msg_maybe = Err(truncation_msg_future);

    // check Discord's 2000 char message limit first
    if text_body.len() + text_end.len() > 2000 {
        let truncation_msg = match truncation_msg_maybe {
            Ok(msg) => msg,
            Err(future) => future.await,
        };

        // This is how long the text body may be at max to conform to Discord's limit
        let available_space =
            2000_usize.saturating_sub(text_end.len()).saturating_sub(truncation_msg.len());

        let mut cut_off_point = available_space;
        while !text_body.is_char_boundary(cut_off_point) {
            cut_off_point -= 1;
        }

        text_body = &text_body[..cut_off_point];
        truncation_msg_maybe = Ok(truncation_msg);
    }

    // check number of lines
    let text_body = if text_body.lines().count() > MAX_OUTPUT_LINES {
        truncation_msg_maybe = Ok(match truncation_msg_maybe {
            Ok(msg) => msg,
            Err(future) => future.await,
        });

        text_body.lines().take(MAX_OUTPUT_LINES).collect::<Vec<_>>().join("\n")
    } else {
        text_body.to_owned()
    };

    if let Ok(truncation_msg) = truncation_msg_maybe {
        format!("{}{}{}", text_body, text_end, truncation_msg)
    } else {
        format!("{}{}", text_body, text_end)
    }
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    data: &Data,
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
