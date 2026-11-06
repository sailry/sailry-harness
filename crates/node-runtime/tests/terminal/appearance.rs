use super::*;

fn styled(screen: &Screen, text: &str) -> Option<TextStyle> {
    screen
        .rows
        .iter()
        .flat_map(|line| &line.spans)
        .find(|span| span.text.contains(text) && span.style != TextStyle::default())
        .map(|span| span.style)
}

fn verify_screen(screen: &Screen, appearance: &Appearance) {
    assert_eq!(screen.foreground, appearance.foreground);
    assert_eq!(screen.background, appearance.background);
    assert_eq!(
        styled(screen, "rgb-color").unwrap().foreground,
        Some(Rgb {
            red: 12,
            green: 34,
            blue: 56,
        })
    );
}

pub(super) async fn verify(
    client: &Client,
    info: &Info,
    stream: &mut dyn Subscription,
    projection: &mut Projection,
) {
    #[cfg(unix)]
    let output = "printf '\\033[2J\\033[H\\033[38;2;12;34;56mrgb-color\\033[0m\\n'";
    #[cfg(windows)]
    let output =
        "$e=[char]27; [Console]::Write(\"${e}[2J${e}[H${e}[38;2;12;34;56mrgb-color${e}[0m`n\")";
    command(client, info, output).await;
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            projection.apply(1, stream.next().await.unwrap()).unwrap();
            if projection.snapshot().is_some_and(|snapshot| {
                styled(&snapshot.screen, "rgb-color").is_some_and(|style| {
                    style.foreground
                        == Some(Rgb {
                            red: 12,
                            green: 34,
                            blue: 56,
                        })
                })
            }) {
                break;
            }
        }
    })
    .await
    .expect("styled terminal output deadline");
    for scheme in [ColorScheme::Light, ColorScheme::Dark] {
        let mut appearance = launch(info.worktree.unwrap()).appearance;
        appearance.color_scheme = scheme;
        if scheme == ColorScheme::Light {
            std::mem::swap(&mut appearance.foreground, &mut appearance.background);
        }
        let Output::TerminalSnapshot(updated) = client
            .execute(client.prepare(Command::SetTerminalAppearance {
                terminal: info.id,
                revision: info.revision,
                appearance: appearance.clone(),
            }))
            .await
            .unwrap()
        else {
            panic!("terminal appearance snapshot expected")
        };
        verify_screen(&updated.screen, &appearance);
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                projection.apply(1, stream.next().await.unwrap()).unwrap();
                if projection
                    .snapshot()
                    .is_some_and(|snapshot| snapshot.screen.background == appearance.background)
                {
                    break;
                }
            }
        })
        .await
        .expect("terminal appearance subscription deadline");
        verify_screen(&projection.snapshot().unwrap().screen, &appearance);
    }
}
