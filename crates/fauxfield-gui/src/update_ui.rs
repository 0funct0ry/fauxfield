//! Opt-in update check (M19): a manual *Check for Updates…* action and, when
//! `check_for_updates = true`, one background lookup per day at startup. A newer release shows a
//! status-bar notice that opens the release page; nothing is ever downloaded or installed.

use fauxfield_store::update;
use gpui::Context;

use crate::shell::Shell;

impl Shell {
    /// Look up the latest release off the main thread. `manual` forces a fresh lookup and
    /// reports "up to date" or the error as a toast; the automatic check stays silent.
    pub(crate) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        let current = Self::build_info().version;
        cx.spawn(async move |this, cx| {
            let res = cx
                .background_executor()
                .spawn(
                    async move { update::check(current, update::cache_path().as_deref(), manual) },
                )
                .await;
            let _ = this.update(cx, |this, cx| {
                match res {
                    Ok(info) => {
                        if manual {
                            let msg = if info.update_available {
                                format!("Fauxfield {} is available", info.latest)
                            } else {
                                format!("Fauxfield {current} is up to date")
                            };
                            this.notify(msg, cx);
                        }
                        this.update_notice = info.update_available.then_some(info);
                    }
                    Err(e) if manual => this.notify_error(format!("Update check failed: {e}"), cx),
                    Err(_) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// Startup hook: only when the user opted in.
    pub(crate) fn start_update_check(&mut self, cx: &mut Context<Self>) {
        if !cfg!(test) && update::enabled() {
            self.check_for_updates(false, cx);
        }
    }

    pub(crate) fn open_update_page(&mut self, cx: &mut Context<Self>) {
        let url = self
            .update_notice
            .as_ref()
            .map(|i| i.url.clone())
            .unwrap_or_else(|| update::RELEASES_PAGE.to_string());
        self.open_link(&url, cx);
    }
}
