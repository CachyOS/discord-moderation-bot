use crate::{Context, Error};
use poise::serenity_prelude as serenity;

/// Links to the bot GitHub repo
#[poise::command(
    prefix_command,
    slash_command,
    category = "Miscellaneous",
    discard_spare_arguments
)]
pub async fn source(ctx: Context<'_>) -> Result<(), Error> {
    ctx.say("https://github.com/cachyos/discord-moderation-bot").await?;
    Ok(())
}

/// Show this menu
#[poise::command(prefix_command, slash_command, category = "Miscellaneous", track_edits)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"]
    #[autocomplete = "poise::builtins::autocomplete_command"]
    command: Option<String>,
) -> Result<(), Error> {
    let extra_text_at_bottom = "\
You can still use all commands with `?`, even if it says `/` above.
Type ?help command for more info on a command.
You can edit your message to the bot and the bot will edit its response.";

    poise::builtins::help(ctx, command.as_deref(), poise::builtins::HelpConfiguration {
        extra_text_at_bottom,
        ephemeral: true,
        ..Default::default()
    })
    .await?;
    Ok(())
}

/// Register slash commands in this guild or globally
///
/// Run with no arguments to register in guild, run with argument "global" to register globally.
#[poise::command(
    prefix_command,
    hide_in_help,
    category = "Miscellaneous",
    check = "crate::checks::check_is_moderator"
)]
pub async fn register(ctx: Context<'_>, #[flag] global: bool) -> Result<(), Error> {
    poise::builtins::register_application_commands(ctx, global).await?;

    Ok(())
}

/// Tells you how long the bot has been up for
#[poise::command(prefix_command, slash_command, hide_in_help, category = "Miscellaneous")]
pub async fn uptime(ctx: Context<'_>) -> Result<(), Error> {
    let uptime = std::time::Instant::now() - ctx.data().bot_start_time;

    let div_mod = |a, b| (a / b, a % b);

    let seconds = uptime.as_secs();
    let (minutes, seconds) = div_mod(seconds, 60);
    let (hours, minutes) = div_mod(minutes, 60);
    let (days, hours) = div_mod(hours, 24);

    ctx.say(format!("Uptime: {}d {}h {}m {}s", days, hours, minutes, seconds)).await?;

    Ok(())
}

/// List servers of which the bot is a member of
#[poise::command(
    slash_command,
    prefix_command,
    track_edits,
    hide_in_help,
    category = "Miscellaneous"
)]
pub async fn servers(ctx: Context<'_>) -> Result<(), Error> {
    poise::builtins::servers(ctx).await?;

    Ok(())
}

/// Displays the SHA-1 git revision the bot was built against
#[poise::command(prefix_command, hide_in_help, discard_spare_arguments, category = "Miscellaneous")]
pub async fn revision(ctx: Context<'_>) -> Result<(), Error> {
    let rustbot_rev: Option<&'static str> = option_env!("RUSTBOT_REV");
    ctx.say(format!("`{}`", rustbot_rev.unwrap_or("unknown"))).await?;
    Ok(())
}

/// Use this joke command to have Conrad Ludgate tell you to get something
///
/// Example: `?conradluget a better computer`
#[poise::command(
    prefix_command,
    slash_command,
    category = "Miscellaneous",
    track_edits,
    hide_in_help
)]
pub async fn conradluget(
    ctx: Context<'_>,
    #[description = "Get what?"]
    #[rest]
    text: String,
) -> Result<(), Error> {
    use once_cell::sync::Lazy;
    static BASE_IMAGE: Lazy<image::DynamicImage> = Lazy::new(|| {
        image::io::Reader::with_format(
            std::io::Cursor::new(&include_bytes!("../assets/conrad.png")[..]),
            image::ImageFormat::Png,
        )
        .decode()
        .expect("failed to load image")
    });
    static FONT: Lazy<ab_glyph::FontRef> = Lazy::new(|| {
        ab_glyph::FontRef::try_from_slice(include_bytes!("../assets/OpenSans.ttf"))
            .expect("failed to load font")
    });

    let text = format!("Get {}", text);

    let image = imageproc::drawing::draw_text(
        &*BASE_IMAGE,
        image::Rgba([201, 209, 217, 255]),
        57,
        286,
        65.0,
        &*FONT,
        &text,
    );

    let mut img_bytes = Vec::with_capacity(200_000); // preallocate 200kB for the img
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut img_bytes), image::ImageFormat::Png)?;

    let filename = text + ".png";

    let attachment = serenity::CreateAttachment::bytes(img_bytes, filename);

    ctx.channel_id().send_files(ctx, vec![attachment], serenity::CreateMessage::new()).await?;

    Ok(())
}

/// Use this joke command to have Nico tell you something
///
/// Example: `?nicosay Get a better computer`
#[poise::command(
    prefix_command,
    slash_command,
    hide_in_help,
    track_edits,
    category = "Miscellaneous"
)]
pub async fn nicosay(
    ctx: Context<'_>,
    #[description = "Say what?"]
    #[rest]
    text: String,
) -> Result<(), Error> {
    use once_cell::sync::Lazy;
    static BASE_IMAGE: Lazy<image::DynamicImage> = Lazy::new(|| {
        image::io::Reader::with_format(
            std::io::Cursor::new(&include_bytes!("../assets/nico.png")[..]),
            image::ImageFormat::Png,
        )
        .decode()
        .expect("failed to load image")
    });
    static FONT: Lazy<ab_glyph::FontRef> = Lazy::new(|| {
        ab_glyph::FontRef::try_from_slice(include_bytes!("../assets/OpenSans.ttf"))
            .expect("failed to load font")
    });

    let image = imageproc::drawing::draw_text(
        &*BASE_IMAGE,
        image::Rgba([201, 209, 217, 255]),
        170,
        13,
        28.0,
        &*FONT,
        &text.to_string(),
    );

    let mut img_bytes = Vec::with_capacity(200_000); // preallocate 200kB for the img
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut std::io::Cursor::new(&mut img_bytes), image::ImageFormat::Png)?;

    let filename = text + ".png";

    let attachment = serenity::CreateAttachment::bytes(img_bytes, filename);

    ctx.channel_id().send_files(ctx, vec![attachment], serenity::CreateMessage::new()).await?;

    Ok(())
}
