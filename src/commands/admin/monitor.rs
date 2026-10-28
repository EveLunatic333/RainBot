use crate::{Components, MyErr, SlashBundle, MONITOR};

#[hertz::hertz_auto]
async fn slash(bnd: &SlashBundle<'_>) -> Result<(), MyErr> {
    if !crate::commands::admin::execute_guard(bnd).await {
        return Ok(());
    }
    let mut mon = MONITOR.lock().await;
    let res = if *mon { "monitor is disabled" } else { "monitor is enabled" };
    *mon = !*mon;
    Components::response(bnd, res, false).await?;
    Ok(())
}