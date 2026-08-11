use super::{theme, App, LogLevel};
use confsync_core::discover::{self, Verdict};
use confsync_core::job::Command;
use confsync_core::paths;
use confsync_core::restore::{self, Action};
use confsync_core::scan::SkipReason;
use confsync_core::settings;
use std::path::PathBuf;

// --- çerçeve parçaları ---------------------------------------------------

pub fn header(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        theme::brand_mark(ui);
        ui.add_space(2.0);
        ui.label(
            egui::RichText::new("confsync")
                .size(19.0)
                .color(theme::TEXT),
        );
        ui.add_space(6.0);
        theme::badge(ui, &app.settings.profile, theme::ACCENT_HOVER);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if app.busy {
                if ui.add(theme::danger_button("İptal")).clicked() {
                    app.worker.cancel();
                }
            } else if ui
                .add(theme::primary_button("Şimdi Yedekle").min_size(egui::vec2(126.0, 30.0)))
                .clicked()
            {
                app.start_backup();
            }

            let has_remote = !app.settings.remote_url.trim().is_empty();
            ui.add_enabled_ui(has_remote && !app.busy, |ui| {
                if ui.add(theme::ghost_button("Push")).clicked() {
                    app.worker.send(Command::Push(app.settings.clone()));
                    app.busy = true;
                }
                if ui.add(theme::ghost_button("Pull")).clicked() {
                    app.worker.send(Command::Pull(app.settings.clone()));
                    app.busy = true;
                }
            });
        });
    });
}

pub fn status_bar(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if app.busy {
            ui.spinner();
            ui.label(egui::RichText::new(&app.stage).color(theme::TEXT));
            match app.progress {
                Some((index, total)) if total > 0 => {
                    let fraction = index as f32 / total as f32;
                    ui.add(
                        egui::ProgressBar::new(fraction)
                            .desired_width(200.0)
                            .desired_height(10.0)
                            .corner_radius(5)
                            .fill(theme::ACCENT)
                            .text(
                                egui::RichText::new(format!("%{:.0}", fraction * 100.0))
                                    .size(11.0)
                                    .color(theme::TEXT),
                            ),
                    );
                    ui.label(
                        egui::RichText::new(format!("{index}/{total}"))
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
                // Tarama aşamasında toplam bilinmiyor: belirsiz çubuk.
                Some((index, _)) => {
                    ui.add(
                        egui::ProgressBar::new(0.0)
                            .desired_width(200.0)
                            .desired_height(10.0)
                            .corner_radius(5)
                            .fill(theme::ACCENT)
                            .animate(true),
                    );
                    ui.label(
                        egui::RichText::new(format!("{index} dosya"))
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
                None => {}
            }
            ui.label(
                egui::RichText::new(shorten(&app.current_file, 56))
                    .size(12.0)
                    .color(theme::FAINT),
            );
        } else {
            let (level, msg) = app
                .log
                .last()
                .map(|(level, msg)| (*level, msg.clone()))
                .unwrap_or((LogLevel::Info, "Hazır".into()));
            theme::dot(ui, level_color(level));
            ui.label(egui::RichText::new(msg).size(12.5).color(theme::MUTED));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(paths::display_short(
                    &app.settings.repo_path,
                    &settings::home_dir(),
                ))
                .size(12.0)
                .color(theme::FAINT),
            );
        });
    });
}

// --- Yedekleme onayı -----------------------------------------------------

/// Yedeklemeden önce açılan onay penceresi: neyin alınacağı, neyin
/// sorulduğu ve neyin elendiği tek ekranda görünür.
pub fn review_window(app: &mut App, ctx: &egui::Context) {
    if app.review.is_none() {
        return;
    }
    let home = settings::home_dir();

    let mut start = false;
    let mut cancel = false;
    // Pencere içeriği `app.review`'i ödünç alacağı için plan geçici olarak
    // dışarı alınır; kararlar üzerinde doğrudan çalışılır.
    let mut plan = app.review.take().expect("yukarıda kontrol edildi");

    egui::Window::new("Yedeklemeden önce onay")
        .collapsible(false)
        .resizable(true)
        .default_width(720.0)
        .max_height(620.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .frame(
            egui::Frame::NONE
                .fill(theme::PANEL)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(12))
                .inner_margin(egui::Margin::symmetric(18, 16))
                .shadow(egui::epaint::Shadow {
                    offset: [0, 10],
                    blur: 28,
                    spread: 0,
                    color: egui::Color32::from_black_alpha(150),
                }),
        )
        .show(ctx, |ui| {
            let questions: Vec<usize> = plan
                .entries
                .iter()
                .enumerate()
                .filter(|(_, e)| e.question.is_some())
                .map(|(i, _)| i)
                .collect();

            ui.horizontal(|ui| {
                theme::badge(
                    ui,
                    &format!("{} dosya alınacak", plan.included_count()),
                    theme::SUCCESS,
                );
                theme::badge(
                    ui,
                    &human_bytes(plan.included_bytes()),
                    theme::ACCENT_HOVER,
                );
                if !questions.is_empty() {
                    theme::badge(
                        ui,
                        &format!("{} karar bekliyor", questions.len()),
                        theme::WARN,
                    );
                }
                if !plan.skipped.is_empty() {
                    theme::badge(
                        ui,
                        &format!("{} elendi", plan.skipped.len()),
                        theme::FAINT,
                    );
                }
            });

            ui.add_space(12.0);

            egui::ScrollArea::vertical()
                .max_height(420.0)
                .id_salt("onay")
                .show(ui, |ui| {
                    if !questions.is_empty() {
                        questions_section(ui, &mut plan, &questions, &home);
                        ui.add_space(12.0);
                    }
                    included_section(app, ui, &mut plan, &home);
                    if !plan.skipped.is_empty() {
                        ui.add_space(12.0);
                        skipped_section(ui, &plan, &home);
                    }
                });

            ui.add_space(12.0);
            theme::hairline(ui);
            ui.add_space(12.0);

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut app.remember_decisions,
                    "Bu kararları hatırla (bir daha sorma)",
                )
                .on_hover_text(
                    "Kararlar ayarlara yazılır; aynı dosyalar sonraki \
                     yedeklemelerde sorulmadan uygulanır.",
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(
                            theme::primary_button(&format!(
                                "{} dosyayı yedekle",
                                plan.included_count()
                            ))
                            .min_size(egui::vec2(160.0, 30.0)),
                        )
                        .clicked()
                    {
                        start = true;
                    }
                    if ui.add(theme::ghost_button("Vazgeç")).clicked() {
                        cancel = true;
                    }
                });
            });
        });

    app.review = Some(plan);
    if start {
        app.apply_review();
    } else if cancel {
        app.cancel_review();
    }
}

fn questions_section(
    ui: &mut egui::Ui,
    plan: &mut confsync_core::backup::BackupPlan,
    questions: &[usize],
    home: &std::path::Path,
) {
    theme::notice(ui, theme::WARN, |ui| {
        ui.label(
            egui::RichText::new("Bu dosyalar için kararınız gerekiyor")
                .size(13.5)
                .color(theme::WARN),
        );
        ui.label(
            egui::RichText::new(
                "Sır içerdiği düşünülen dosyalar işaretlenirse depoya —uzak depo \
                 tanımlıysa oraya da— olduğu gibi gider. Emin değilseniz dışarıda bırakın.",
            )
            .size(12.5)
            .color(theme::MUTED),
        );
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if ui.add(theme::ghost_button("Hepsini al")).clicked() {
                plan.set_all_questions(true);
            }
            if ui.add(theme::ghost_button("Hiçbirini alma")).clicked() {
                plan.set_all_questions(false);
            }
        });
    });

    ui.add_space(8.0);

    for &index in questions {
        let entry = &mut plan.entries[index];
        let question = entry.question.clone().expect("soru olan girdiler");
        theme::inset().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.checkbox(&mut entry.include, "");
                ui.vertical(|ui| {
                    ui.label(
                        egui::RichText::new(paths::display_short(&entry.item.path, home))
                            .size(13.0)
                            .color(theme::TEXT),
                    );
                    let detail = question
                        .detail
                        .as_deref()
                        .map(|d| format!(" — {d}"))
                        .unwrap_or_default();
                    ui.label(
                        egui::RichText::new(format!(
                            "{}{detail} · {}",
                            question.reason.label(),
                            human_bytes(entry.item.size)
                        ))
                        .size(12.0)
                        .color(theme::FAINT),
                    );
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if entry.include {
                        theme::badge(ui, "yedeğe girecek", theme::SUCCESS);
                    } else {
                        theme::badge(ui, "atlanacak", theme::MUTED);
                    }
                });
            });
        });
        ui.add_space(4.0);
    }
}

fn included_section(
    app: &mut App,
    ui: &mut egui::Ui,
    plan: &mut confsync_core::backup::BackupPlan,
    home: &std::path::Path,
) {
    let plain: Vec<usize> = plan
        .entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.question.is_none())
        .map(|(i, _)| i)
        .collect();

    let label = format!("Sorusuz alınacaklar ({})", plain.len());
    let header = egui::CollapsingHeader::new(
        egui::RichText::new(label).size(13.0).color(theme::MUTED),
    )
    .open(Some(app.show_included))
    .show(ui, |ui| {
        ui.label(
            egui::RichText::new("İşareti kaldırılan dosya bu yedeğe girmez.")
                .size(12.0)
                .color(theme::FAINT),
        );
        ui.add_space(6.0);

        // 600+ satır olabiliyor; yalnızca görünen satırlar çizilir.
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 6.0;
        egui::ScrollArea::vertical()
            .max_height(260.0)
            .id_salt("dahil_edilecekler")
            .show_rows(ui, row_height, plain.len(), |ui, range| {
                for row in range {
                    let entry = &mut plan.entries[plain[row]];
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut entry.include, "");
                        ui.label(
                            egui::RichText::new(paths::display_short(&entry.item.path, home))
                                .size(12.5)
                                .color(if entry.include {
                                    theme::TEXT
                                } else {
                                    theme::FAINT
                                }),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(human_bytes(entry.item.size))
                                        .size(11.5)
                                        .color(theme::FAINT),
                                );
                            },
                        );
                    });
                }
            });
    });
    if header.header_response.clicked() {
        app.show_included = !app.show_included;
    }
}

fn skipped_section(
    ui: &mut egui::Ui,
    plan: &confsync_core::backup::BackupPlan,
    home: &std::path::Path,
) {
    egui::CollapsingHeader::new(
        egui::RichText::new(format!("Kalıp/teknik nedenle elenenler ({})", plan.skipped.len()))
            .size(13.0)
            .color(theme::MUTED),
    )
    .show(ui, |ui| {
        ui.label(
            egui::RichText::new(
                "Bunlar karar dışıdır: hariç tutma kalıpları, okunamayan dosyalar \
                 ve iç içe git depoları.",
            )
            .size(12.0)
            .color(theme::FAINT),
        );
        ui.add_space(6.0);
        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        egui::ScrollArea::vertical()
            .max_height(200.0)
            .id_salt("elenenler")
            .show_rows(ui, row_height, plan.skipped.len(), |ui, range| {
                for row in range {
                    let skipped = &plan.skipped[row];
                    ui.label(
                        egui::RichText::new(format!(
                            "{}  —  {}",
                            paths::display_short(&skipped.path, home),
                            skipped.reason.label()
                        ))
                        .size(12.0)
                        .color(theme::FAINT),
                    );
                }
            });
    });
}

// --- Genel Bakış ---------------------------------------------------------

pub fn overview(app: &mut App, ui: &mut egui::Ui) {
    let home = settings::home_dir();

    theme::title(
        ui,
        "Genel Bakış",
        "Deponun durumu ve son yedekleme işleminin özeti.",
    );

    // Toplamlar için gereken ölçümler (arka planda, tekrarsız).
    let repo_path = app.settings.repo_path.clone();
    let mut wanted: Vec<PathBuf> = app
        .settings
        .enabled_sources()
        .map(|s| s.path.clone())
        .collect();
    wanted.push(repo_path.clone());
    app.ensure_sizes(wanted);

    if app.busy {
        progress_card(app, ui);
        ui.add_space(14.0);
    }

    let enabled = app.settings.enabled_sources().count();
    let patterns = app
        .settings
        .excludes
        .iter()
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .count();

    let (sources_total, complete) = app.enabled_sources_total();
    let repo_size = app.sizes.get(&repo_path).copied();

    // Ölçüm tamamlanmadıysa toplam alt sınırdır; "≈" bunu belli eder.
    let tiles: [(String, &str, egui::Color32); 5] = [
        (
            format!("{enabled}/{}", app.settings.sources.len()),
            "etkin kaynak",
            theme::TEXT,
        ),
        (
            format!(
                "{}{}",
                if complete { "" } else { "≈" },
                human_bytes(sources_total)
            ),
            "kaynak toplamı",
            theme::TEXT,
        ),
        (
            repo_size.map(human_bytes).unwrap_or_else(|| "…".into()),
            "depo boyutu",
            theme::ACCENT_HOVER,
        ),
        (patterns.to_string(), "hariç tutma kalıbı", theme::TEXT),
        match &app.last_backup {
            Some(report) => (
                report.stored.to_string(),
                "son yedekte dosya",
                theme::SUCCESS,
            ),
            None => (app.history.len().to_string(), "commit", theme::TEXT),
        },
    ];

    // `horizontal_wrapped` kart çerçevelerini sarmalamıyor, dar pencerede
    // son kutucuk kırpılıyordu; sütunlar alanı eşit paylaştırıyor.
    ui.columns(tiles.len(), |cols| {
        for (col, (value, label, tint)) in cols.iter_mut().zip(tiles.iter()) {
            theme::stat_tile(col, value, label, *tint);
        }
    });

    ui.add_space(14.0);
    changes_table(app, ui, &home);

    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::caption(ui, "DEPO");
        ui.add_space(6.0);
        info_row(ui, "Yerel", &app.settings.repo_path.display().to_string());
        info_row(
            ui,
            "Uzak",
            if app.settings.remote_url.trim().is_empty() {
                "tanımlı değil (yalnızca yerel)"
            } else {
                &app.settings.remote_url
            },
        );
        info_row(ui, "Dal", &app.settings.branch);
    });

    if let Some(report) = &app.last_backup {
        ui.add_space(12.0);
        let skipped: Vec<(String, bool)> = report
            .scan
            .skipped
            .iter()
            .map(|s| {
                (
                    format!(
                        "{}  —  {}{}",
                        paths::display_short(&s.path, &home),
                        s.reason.label(),
                        s.detail
                            .as_ref()
                            .map(|d| format!(" ({d})"))
                            .unwrap_or_default()
                    ),
                    s.reason == SkipReason::Secret,
                )
            })
            .collect();

        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::caption(ui, "SON YEDEK");
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(format!(
                    "{} dosya saklandı · {} atlandı · {}",
                    report.stored,
                    report.skipped,
                    human_bytes(report.bytes)
                ))
                .color(theme::TEXT),
            );

            if !skipped.is_empty() {
                ui.add_space(8.0);
                egui::CollapsingHeader::new(
                    egui::RichText::new(format!("Atlanan dosyalar ({})", skipped.len()))
                        .size(13.0)
                        .color(theme::MUTED),
                )
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(200.0)
                        .id_salt("atlananlar")
                        .show(ui, |ui| {
                            for (line, is_secret) in &skipped {
                                let color = if *is_secret {
                                    theme::WARN
                                } else {
                                    theme::FAINT
                                };
                                ui.label(egui::RichText::new(line).size(12.5).color(color));
                            }
                        });
                });
            }
        });
    }

    ui.add_space(12.0);
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::caption(ui, "GÜNLÜK");
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .max_height(180.0)
            .id_salt("gunluk")
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for (level, msg) in &app.log {
                    ui.label(
                        egui::RichText::new(msg)
                            .size(12.5)
                            .color(level_color(*level)),
                    );
                }
            });
    });
}

/// Yedekleme/geri yükleme sürerken Genel Bakış'ın tepesindeki ilerleme kartı.
/// Durum çubuğundaki ince gösterge yerine yüzdeyi büyük biçimde verir.
fn progress_card(app: &App, ui: &mut egui::Ui) {
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        let fraction = match app.progress {
            Some((index, total)) if total > 0 => Some(index as f32 / total as f32),
            _ => None,
        };

        ui.horizontal(|ui| {
            ui.spinner();
            ui.label(
                egui::RichText::new(if app.stage.is_empty() {
                    "Çalışıyor"
                } else {
                    &app.stage
                })
                .size(14.5)
                .color(theme::TEXT),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                match fraction {
                    Some(fraction) => ui.label(
                        egui::RichText::new(format!("%{:.0}", fraction * 100.0))
                            .size(20.0)
                            .color(theme::ACCENT_HOVER),
                    ),
                    // Tarama aşamasında toplam bilinmediği için yüzde yok.
                    None => ui.label(
                        egui::RichText::new("hazırlanıyor")
                            .size(12.5)
                            .color(theme::MUTED),
                    ),
                };
            });
        });

        ui.add_space(10.0);
        let mut bar = egui::ProgressBar::new(fraction.unwrap_or(0.0))
            .desired_width(ui.available_width())
            .desired_height(12.0)
            .corner_radius(6)
            .fill(theme::ACCENT);
        if fraction.is_none() {
            bar = bar.animate(true);
        }
        ui.add(bar);

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(shorten(&app.current_file, 64))
                    .size(12.0)
                    .color(theme::FAINT),
            );
            if let Some((index, total)) = app.progress {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let text = if total > 0 {
                        format!("{index} / {total} dosya")
                    } else {
                        format!("{index} dosya tarandı")
                    };
                    ui.label(egui::RichText::new(text).size(12.0).color(theme::MUTED));
                });
            }
        });
    });
}

/// Son yedeğe göre değişen dosyalar tablosu.
///
/// Satırlar sanallaştırılmış çizilir: yeni bir kaynak eklendiğinde binlerce
/// dosya "yeni" görünebiliyor, tümünü çizmek kareyi düşürürdü.
fn changes_table(app: &mut App, ui: &mut egui::Ui, home: &std::path::Path) {
    use confsync_core::backup::ChangeKind;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());

        ui.horizontal(|ui| {
            let title = match &app.changes {
                Some(report) if !report.is_empty() => {
                    format!("DEĞİŞENLER ({})", report.total())
                }
                _ => "DEĞİŞENLER".to_string(),
            };
            theme::caption(ui, &title);

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add_enabled(!app.detecting, theme::ghost_button("Yenile"))
                    .on_hover_text("Kaynakları son yedekle yeniden karşılaştırır")
                    .clicked()
                {
                    app.start_detect_changes();
                }
                if app.detecting {
                    ui.spinner();
                    ui.label(
                        egui::RichText::new("karşılaştırılıyor…")
                            .size(12.0)
                            .color(theme::MUTED),
                    );
                }
            });
        });

        ui.add_space(8.0);

        let Some(report) = &app.changes else {
            ui.label(
                egui::RichText::new("Karşılaştırma henüz yapılmadı.")
                    .size(12.5)
                    .color(theme::FAINT),
            );
            return;
        };

        if report.is_empty() {
            ui.horizontal(|ui| {
                theme::dot(ui, theme::SUCCESS);
                ui.label(
                    egui::RichText::new("Kaynaklar son yedekle aynı.")
                        .size(13.0)
                        .color(theme::MUTED),
                );
            });
            return;
        }

        ui.horizontal(|ui| {
            for kind in [ChangeKind::Added, ChangeKind::Modified, ChangeKind::Removed] {
                let count = report.count(kind);
                if count > 0 {
                    theme::badge(ui, &format!("{count} {}", kind.label()), change_color(kind));
                }
            }
            theme::badge(ui, &human_bytes(report.bytes()), theme::FAINT);
            if report.questions > 0 {
                theme::badge(
                    ui,
                    &format!("{} karar bekliyor", report.questions),
                    theme::WARN,
                );
            }
        });

        ui.add_space(10.0);

        // Başlık satırı.
        ui.horizontal(|ui| {
            ui.add_sized(
                [88.0, 16.0],
                egui::Label::new(egui::RichText::new("DURUM").size(11.0).color(theme::FAINT)),
            );
            ui.label(egui::RichText::new("DOSYA").size(11.0).color(theme::FAINT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(egui::RichText::new("BOYUT").size(11.0).color(theme::FAINT));
            });
        });
        theme::hairline(ui);
        ui.add_space(4.0);

        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 8.0;
        egui::ScrollArea::vertical()
            .max_height(280.0)
            .id_salt("degisenler")
            .show_rows(ui, row_height, report.files.len(), |ui, range| {
                for row in range {
                    let file = &report.files[row];
                    ui.horizontal(|ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(88.0, row_height),
                            egui::Layout::left_to_right(egui::Align::Center),
                            |ui| {
                                theme::badge(ui, file.kind.label(), change_color(file.kind));
                            },
                        );
                        ui.label(
                            egui::RichText::new(paths::display_short(&file.path, home))
                                .size(12.5)
                                .color(if file.kind == ChangeKind::Removed {
                                    theme::FAINT
                                } else {
                                    theme::TEXT
                                }),
                        );
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                ui.label(
                                    egui::RichText::new(human_bytes(file.size))
                                        .size(12.0)
                                        .color(theme::FAINT),
                                );
                            },
                        );
                    });
                }
            });
    });
}

fn change_color(kind: confsync_core::backup::ChangeKind) -> egui::Color32 {
    use confsync_core::backup::ChangeKind;
    match kind {
        ChangeKind::Added => theme::SUCCESS,
        ChangeKind::Modified => theme::ACCENT_HOVER,
        ChangeKind::Removed => theme::DANGER,
    }
}

fn info_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [110.0, 18.0],
            egui::Label::new(
                egui::RichText::new(label)
                    .size(13.0)
                    .color(theme::MUTED),
            )
            .halign(egui::Align::LEFT),
        );
        ui.label(egui::RichText::new(value).size(13.0).color(theme::TEXT));
    });
}

// --- Kaynaklar -----------------------------------------------------------

pub fn sources(app: &mut App, ui: &mut egui::Ui) {
    let home = settings::home_dir();

    theme::title(
        ui,
        "Yedeklenecek Kaynaklar",
        "Yalnızca yapılandırma dosyalarını taşıyın; uygulama verisi yedeğe girmesin.",
    );

    // Eski ayarlardan gelen "~/.config'in tamamı" kaynağı gerçek bir tuzak:
    // tarayıcı profilleri yüzlerce megabayt tutuyor.
    let broad = app
        .settings
        .sources
        .iter()
        .position(|s| discover::is_whole_config_dir(&s.path, &home));
    if let Some(index) = broad {
        theme::notice(ui, theme::WARN, |ui| {
            ui.label(
                egui::RichText::new("~/.config bütün olarak ekli")
                    .size(13.5)
                    .color(theme::WARN),
            );
            ui.label(
                egui::RichText::new(
                    "Bu klasörde tarayıcı profilleri ve uygulama durum dosyaları da bulunur; \
                     yedek gereksiz yere büyür. Aşağıdaki keşif paneliyle yalnızca gerçek \
                     yapılandırma klasörlerini seçebilirsiniz.",
                )
                .size(12.5)
                .color(theme::MUTED),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .add(theme::primary_button("Bunun yerine önerilenleri kullan"))
                    .clicked()
                {
                    app.settings.sources.remove(index);
                    let mut added = 0;
                    for path in discover::recommended_paths(&home) {
                        if app.settings.add_source(path) {
                            added += 1;
                        }
                    }
                    app.settings_dirty = true;
                    app.info(format!(
                        "~/.config kaynak listesinden çıkarıldı; {added} bilinen \
                         yapılandırma girdisi eklendi."
                    ));
                }
                if ui.add(theme::ghost_button("Olduğu gibi bırak")).clicked() {
                    app.warn(
                        "~/.config bütün olarak taranmaya devam edecek. \
                         Hariç tutma kalıpları ağır klasörleri yine de eleyecektir.",
                    );
                }
            });
        });
        ui.add_space(12.0);
    }

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            let response = ui.add(
                egui::TextEdit::singleline(&mut app.new_source_input)
                    .hint_text("~/.config/nvim ya da /etc/nginx")
                    .desired_width(300.0),
            );
            let submitted = response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

            if ui.add(theme::primary_button("Ekle")).clicked() || submitted {
                let raw = app.new_source_input.clone();
                add_source(app, paths::expand_tilde(&raw, &home));
                app.new_source_input.clear();
            }
            if ui.add(theme::ghost_button("Klasör seç…")).clicked() {
                if let Some(dir) = rfd::FileDialog::new().set_directory(&home).pick_folder() {
                    add_source(app, dir);
                }
            }
            if ui.add(theme::ghost_button("Dosya seç…")).clicked() {
                if let Some(file) = rfd::FileDialog::new().set_directory(&home).pick_file() {
                    add_source(app, file);
                }
            }
        });
    });

    ui.add_space(12.0);

    let mut remove: Option<usize> = None;
    let mut changed = false;

    // Boyutlar arka planda ölçülür; satırlar ölçülene dek "…" gösterir.
    let source_paths: Vec<PathBuf> = app.settings.sources.iter().map(|s| s.path.clone()).collect();
    app.ensure_sizes(source_paths.clone());
    let size_labels: Vec<String> = source_paths
        .iter()
        .map(|path| {
            app.sizes
                .get(path)
                .map(|bytes| human_bytes(*bytes))
                .unwrap_or_else(|| "…".into())
        })
        .collect();

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            theme::caption(ui, &format!("LİSTE ({})", app.settings.sources.len()));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(theme::ghost_button("Boyutları yenile"))
                    .on_hover_text("Diskteki güncel boyutları yeniden ölçer")
                    .clicked()
                {
                    for path in &source_paths {
                        app.sizes.remove(path);
                    }
                }
            });
        });
        ui.add_space(8.0);

        if app.settings.sources.is_empty() {
            ui.label(
                egui::RichText::new("Henüz kaynak yok.")
                    .size(13.0)
                    .color(theme::FAINT),
            );
        }

        // Liste uzayınca altındaki keşif paneli erişilemez hale geliyordu;
        // bu yüzden liste kendi içinde kaydırılır.
        egui::ScrollArea::vertical()
            .max_height(300.0)
            .id_salt("kaynak_listesi")
            .show(ui, |ui| {
                for (index, source) in app.settings.sources.iter_mut().enumerate() {
                    theme::inset().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            if ui.checkbox(&mut source.enabled, "").changed() {
                                changed = true;
                            }
                            let text = paths::display_short(&source.path, &home);
                            let exists = source.path.exists();
                            ui.label(egui::RichText::new(text).size(13.5).color(
                                if source.enabled {
                                    theme::TEXT
                                } else {
                                    theme::FAINT
                                },
                            ));
                            if !exists {
                                theme::badge(ui, "bulunamadı", theme::DANGER);
                            }
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if theme::close_button(ui)
                                        .on_hover_text("Listeden çıkar")
                                        .clicked()
                                    {
                                        remove = Some(index);
                                    }
                                    ui.label(
                                        egui::RichText::new(&size_labels[index])
                                            .size(12.0)
                                            .color(theme::MUTED),
                                    )
                                    .on_hover_text(
                                        "Diskteki boyut (hariç tutma kalıpları uygulanmadan)",
                                    );
                                },
                            );
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    });

    if let Some(index) = remove {
        app.settings.sources.remove(index);
        changed = true;
    }
    if changed {
        app.settings_dirty = true;
    }

    ui.add_space(12.0);
    discovery_panel(app, ui, &home);
    ui.add_space(12.0);
    save_row(app, ui);
}

/// `~/.config` altındaki girdileri ölçüp öneri olarak listeler.
fn discovery_panel(app: &mut App, ui: &mut egui::Ui, home: &std::path::Path) {
    // Liste render edilirken `app`'in geri kalanına da erişmek gerekiyor;
    // ödünç çakışmasını önlemek için geçici olarak dışarı alınır.
    let discovery = std::mem::take(&mut app.discovery);
    let mut to_add: Vec<PathBuf> = Vec::new();

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            theme::caption(ui, "~/.CONFIG KEŞFİ");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = if discovery.is_empty() {
                    "İncele"
                } else {
                    "Yeniden incele"
                };
                if ui
                    .add_enabled(!app.busy, theme::ghost_button(label))
                    .clicked()
                {
                    app.start_discovery();
                }
                if !discovery.is_empty() {
                    ui.checkbox(&mut app.show_heavy, "ağırları da göster");
                }
            });
        });

        ui.add_space(6.0);

        if discovery.is_empty() {
            ui.label(
                egui::RichText::new(
                    "~/.config içindeki her girdinin boyutunu ölçüp hangilerinin gerçek \
                     yapılandırma olduğunu ayırır. Tarayıcı profilleri gibi ağır klasörler \
                     işaretlenir, önerilenleri tek tıkla ekleyebilirsiniz.",
                )
                .size(12.5)
                .color(theme::MUTED),
            );
            // Liste boş; aşağıdaki geri yazma closure'dan sonra yapılır.
            return;
        }

        let recommended: Vec<&discover::Candidate> = discovery
            .iter()
            .filter(|c| c.verdict == Verdict::Recommended)
            .filter(|c| !app.settings.sources.iter().any(|s| s.path == c.path))
            .collect();

        if !recommended.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{} önerilen girdi henüz listede değil.",
                        recommended.len()
                    ))
                    .size(12.5)
                    .color(theme::MUTED),
                );
                if ui
                    .add(theme::primary_button("Önerilenlerin hepsini ekle"))
                    .clicked()
                {
                    to_add.extend(recommended.iter().map(|c| c.path.clone()));
                }
            });
            ui.add_space(8.0);
        }

        egui::ScrollArea::vertical()
            .max_height(320.0)
            .id_salt("kesif")
            .show(ui, |ui| {
                for candidate in &discovery {
                    if candidate.verdict == Verdict::Heavy && !app.show_heavy {
                        continue;
                    }
                    let already = app.settings.sources.iter().any(|s| s.path == candidate.path);
                    theme::inset().show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(
                                egui::RichText::new(&candidate.name)
                                    .size(13.5)
                                    .color(if candidate.verdict == Verdict::Heavy {
                                        theme::FAINT
                                    } else {
                                        theme::TEXT
                                    }),
                            );
                            theme::badge(
                                ui,
                                candidate.verdict.label(),
                                verdict_color(candidate.verdict),
                            );
                            ui.label(
                                egui::RichText::new(format!(
                                    "{}{} · {} dosya · {}",
                                    if candidate.truncated { "≥" } else { "" },
                                    human_bytes(candidate.size),
                                    candidate.files,
                                    candidate.reason
                                ))
                                .size(12.0)
                                .color(theme::FAINT),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if already {
                                        ui.label(
                                            egui::RichText::new("listede")
                                                .size(12.0)
                                                .color(theme::SUCCESS),
                                        );
                                    } else if ui
                                        .add(theme::ghost_button("Ekle"))
                                        .clicked()
                                    {
                                        to_add.push(candidate.path.clone());
                                    }
                                },
                            );
                        });
                    });
                    ui.add_space(4.0);
                }
            });
    });

    app.discovery = discovery;

    for path in to_add {
        if app.settings.add_source(path.clone()) {
            app.settings_dirty = true;
            app.info(format!("Eklendi: {}", paths::display_short(&path, home)));
        }
    }
}

fn verdict_color(verdict: Verdict) -> egui::Color32 {
    match verdict {
        Verdict::Recommended => theme::SUCCESS,
        Verdict::Optional => theme::ACCENT_HOVER,
        Verdict::Heavy => theme::WARN,
    }
}

fn add_source(app: &mut App, path: PathBuf) {
    let home = settings::home_dir();
    if path.as_os_str().is_empty() {
        return;
    }
    if paths::is_too_broad(&path, &home) {
        app.warn(format!(
            "{} çok geniş bir kapsam; alt klasörleri tek tek eklemeniz önerilir.",
            path.display()
        ));
    }
    if discover::is_whole_config_dir(&path, &home) {
        app.warn(
            "~/.config'in tamamı ekleniyor: içinde tarayıcı profilleri ve uygulama \
             verisi de var. Keşif paneliyle tek tek seçmek çok daha küçük bir yedek üretir.",
        );
    }
    if !path.exists() {
        app.warn(format!("{} şu an mevcut değil, yine de eklendi.", path.display()));
    }
    if app.settings.add_source(path.clone()) {
        app.settings_dirty = true;
        app.info(format!("Eklendi: {}", paths::display_short(&path, &home)));
    } else {
        app.warn("Bu yol zaten listede.");
    }
}

// --- Hariç Tutulanlar ----------------------------------------------------

pub fn excludes(app: &mut App, ui: &mut egui::Ui) {
    theme::title(
        ui,
        "Hariç Tutma Kalıpları",
        "gitignore sözdizimi geçerlidir: `#` yorum, `!` istisna tanımlar.",
    );

    let mut changed = false;
    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        let response = ui.add(
            egui::TextEdit::multiline(&mut app.excludes_text)
                .code_editor()
                .desired_width(f32::INFINITY)
                .desired_rows(20)
                .background_color(theme::SUNKEN),
        );
        if response.changed() {
            changed = true;
        }
    });
    if changed {
        app.sync_excludes_from_text();
    }

    ui.add_space(10.0);
    ui.horizontal(|ui| {
        if ui.add(theme::ghost_button("Varsayılanları geri getir")).clicked() {
            app.excludes_text = settings::default_excludes().join("\n");
            app.sync_excludes_from_text();
            app.info("Varsayılan hariç tutma listesi yüklendi.");
        }
        if ui.add(theme::ghost_button("Eksik varsayılanları ekle")).clicked() {
            let mut lines: Vec<String> = app.excludes_text.lines().map(String::from).collect();
            let mut added = 0;
            for pattern in settings::default_excludes() {
                if !lines.iter().any(|l| l == &pattern) {
                    lines.push(pattern);
                    added += 1;
                }
            }
            app.excludes_text = lines.join("\n");
            app.sync_excludes_from_text();
            app.info(format!("{added} kalıp eklendi."));
        }
    });

    ui.add_space(12.0);
    save_row(app, ui);
}

// --- Geri Yükle ----------------------------------------------------------

pub fn restore(app: &mut App, ui: &mut egui::Ui) {
    theme::title(
        ui,
        "Geri Yükle",
        "Önce kuru çalışma yapılır; hiçbir dosya siz onaylamadan değişmez.",
    );

    let profiles = restore::available_profiles(&app.settings.repo_path);

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Profil").size(13.0).color(theme::MUTED));
            egui::ComboBox::from_id_salt("profil_secimi")
                .selected_text(app.settings.profile.clone())
                .show_ui(ui, |ui| {
                    for profile in &profiles {
                        ui.selectable_value(&mut app.settings.profile, profile.clone(), profile);
                    }
                });

            ui.add_space(12.0);
            ui.label(
                egui::RichText::new("Hedef ev dizini")
                    .size(13.0)
                    .color(theme::MUTED),
            );
            let mut target = app.restore_target_home.display().to_string();
            if ui
                .add(egui::TextEdit::singleline(&mut target).desired_width(220.0))
                .changed()
            {
                app.restore_target_home = PathBuf::from(target);
            }
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!app.busy, theme::primary_button("Planı çıkar (kuru çalışma)"))
                .clicked()
            {
                app.start_plan_restore();
            }
            ui.checkbox(&mut app.make_rollback, "Üzerine yazmadan önce yedek al");
        });
    });

    ui.add_space(12.0);

    if app.plan.is_none() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(20.0);
                ui.label(egui::RichText::new("📥").size(28.0).color(theme::FAINT));
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new("Henüz plan yok")
                        .size(14.0)
                        .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new(
                        "Yukarıdaki düğme, depodaki içeriğin sisteminize ne yapacağını gösterir.",
                    )
                    .size(12.5)
                    .color(theme::FAINT),
                );
                ui.add_space(20.0);
            });
        });
        return;
    }

    // `app.plan` ödünç alınmadan önce gerekli alanların kopyası çıkarılır;
    // aksi halde plan üzerinde çalışırken `app`'in başka alanlarına erişilemez.
    let target_home = app.restore_target_home.clone();
    let busy = app.busy;
    let mut confirm = app.confirm_restore;
    let mut apply_now = false;

    if let Some(plan) = &mut app.plan {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                plan_badge(ui, "oluşturulacak", plan.count(Action::Create), theme::SUCCESS);
                plan_badge(ui, "üzerine yazılacak", plan.count(Action::Overwrite), theme::WARN);
                plan_badge(ui, "değişmedi", plan.count(Action::Unchanged), theme::FAINT);
                plan_badge(ui, "çakışma", plan.count(Action::Conflict), theme::DANGER);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::ghost_button("Hiçbirini seçme")).clicked() {
                        plan.select_all(false);
                    }
                    if ui.add(theme::ghost_button("Tümünü seç")).clicked() {
                        plan.select_all(true);
                    }
                });
            });

            if plan.source_home != target_home.to_string_lossy() {
                ui.add_space(8.0);
                theme::notice(ui, theme::WARN, |ui| {
                    ui.label(
                        egui::RichText::new(format!(
                            "Yedek {} dizininden alınmış, {} dizinine yazılacak.",
                            plan.source_home,
                            target_home.display()
                        ))
                        .size(12.5)
                        .color(theme::WARN),
                    );
                });
            }

            ui.add_space(10.0);
            egui::ScrollArea::vertical()
                .max_height(320.0)
                .id_salt("plan_listesi")
                .show(ui, |ui| {
                    egui::Grid::new("plan")
                        .num_columns(3)
                        .striped(true)
                        .spacing([12.0, 6.0])
                        .show(ui, |ui| {
                            for item in &mut plan.items {
                                let writes = item.action.writes();
                                ui.add_enabled_ui(writes, |ui| {
                                    ui.checkbox(&mut item.selected, "");
                                });
                                ui.label(action_text(item.action));
                                let mut text = item.target.display().to_string();
                                if let Some(note) = &item.note {
                                    text.push_str(&format!("   — {note}"));
                                }
                                ui.label(
                                    egui::RichText::new(text).size(12.5).color(theme::MUTED),
                                );
                                ui.end_row();
                            }
                        });
                });
        });

        ui.add_space(12.0);
        let selected = plan.selected_writes();

        if !confirm {
            if ui
                .add_enabled(
                    selected > 0 && !busy,
                    theme::primary_button(&format!("{selected} dosyayı geri yükle")),
                )
                .clicked()
            {
                confirm = true;
            }
        } else {
            theme::notice(ui, theme::DANGER, |ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{selected} dosyanın üzerine yazılacak. Emin misiniz?"
                    ))
                    .size(13.5)
                    .color(theme::DANGER),
                );
                ui.add_space(6.0);
                ui.horizontal(|ui| {
                    if ui.add(theme::primary_button("Evet, geri yükle")).clicked() {
                        apply_now = true;
                    }
                    if ui.add(theme::ghost_button("Vazgeç")).clicked() {
                        confirm = false;
                    }
                });
            });
        }
    }

    app.confirm_restore = confirm;

    if apply_now {
        if let Some(plan) = app.plan.take() {
            app.busy = true;
            let make_rollback = app.make_rollback;
            app.worker.send(Command::ApplyRestore {
                plan: Box::new(plan),
                make_rollback,
            });
        }
    }
}

fn plan_badge(ui: &mut egui::Ui, label: &str, count: usize, color: egui::Color32) {
    theme::badge(ui, &format!("{count} {label}"), color);
}

// --- Geçmiş --------------------------------------------------------------

pub fn history(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        theme::title(ui, "Geçmiş", "");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add(theme::ghost_button("Yenile")).clicked() {
                app.worker.send(Command::LoadHistory(app.settings.clone()));
            }
        });
    });

    if app.history.is_empty() {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new("Henüz commit yok")
                        .size(14.0)
                        .color(theme::MUTED),
                );
                ui.label(
                    egui::RichText::new("İlk yedeği aldığınızda burada listelenecek.")
                        .size(12.5)
                        .color(theme::FAINT),
                );
                ui.add_space(24.0);
            });
        });
        return;
    }

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        for commit in &app.history {
            theme::inset().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(&commit.short_id)
                            .monospace()
                            .size(12.5)
                            .color(theme::ACCENT_HOVER),
                    );
                    ui.label(
                        egui::RichText::new(&commit.summary)
                            .size(13.0)
                            .color(theme::TEXT),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            egui::RichText::new(commit.local_time())
                                .size(12.0)
                                .color(theme::FAINT),
                        );
                    });
                });
            });
            ui.add_space(4.0);
        }
    });
}

// --- Ayarlar -------------------------------------------------------------

pub fn settings(app: &mut App, ui: &mut egui::Ui) {
    theme::title(ui, "Ayarlar", "Depo, uzak bağlantı ve tarama davranışı.");

    let mut changed = false;

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::caption(ui, "DEPO");
        ui.add_space(8.0);

        egui::Grid::new("ayarlar")
            .num_columns(2)
            .spacing([16.0, 10.0])
            .show(ui, |ui| {
                ui.label(egui::RichText::new("Yerel depo yolu").color(theme::MUTED));
                ui.horizontal(|ui| {
                    let mut text = app.settings.repo_path.display().to_string();
                    if ui
                        .add(egui::TextEdit::singleline(&mut text).desired_width(320.0))
                        .changed()
                    {
                        app.settings.repo_path = PathBuf::from(text);
                        changed = true;
                    }
                    if ui.add(theme::ghost_button("…")).clicked() {
                        if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                            app.settings.repo_path = dir;
                            changed = true;
                        }
                    }
                });
                ui.end_row();

                ui.label(egui::RichText::new("Uzak depo (SSH/HTTPS)").color(theme::MUTED));
                if ui
                    .add(
                        egui::TextEdit::singleline(&mut app.settings.remote_url)
                            .hint_text("git@github.com:kullanici/dotfiles.git")
                            .desired_width(380.0),
                    )
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();

                ui.label(egui::RichText::new("Dal").color(theme::MUTED));
                if ui
                    .add(egui::TextEdit::singleline(&mut app.settings.branch).desired_width(160.0))
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();

                ui.label(egui::RichText::new("Profil (makine adı)").color(theme::MUTED));
                if ui
                    .add(egui::TextEdit::singleline(&mut app.settings.profile).desired_width(220.0))
                    .changed()
                {
                    changed = true;
                }
                ui.end_row();

                ui.label(egui::RichText::new("Commit sahibi").color(theme::MUTED));
                ui.horizontal(|ui| {
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.settings.author_name)
                                .desired_width(150.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .add(
                            egui::TextEdit::singleline(&mut app.settings.author_email)
                                .desired_width(220.0),
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });
                ui.end_row();
            });
    });

    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::caption(ui, "TARAMA");
        ui.add_space(8.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Azami dosya boyutu").color(theme::MUTED));
            if ui
                .add(egui::DragValue::new(&mut app.settings.max_file_size_mb).range(1..=512))
                .changed()
            {
                changed = true;
            }
            ui.label(egui::RichText::new("MiB").size(12.5).color(theme::FAINT));
        });

        ui.add_space(8.0);
        if ui
            .checkbox(
                &mut app.settings.skip_secrets,
                "Sır içerdiği düşünülen dosyaları atla (önerilir)",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut app.settings.follow_symlinks,
                "Sembolik bağlantıların hedefini kopyala",
            )
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(&mut app.settings.auto_push, "Yedekten sonra otomatik push")
            .changed()
        {
            changed = true;
        }
        if ui
            .checkbox(
                &mut app.settings.always_ask,
                "Sorulacak bir şey olmasa da yedekleme öncesi onay penceresini göster",
            )
            .changed()
        {
            changed = true;
        }
    });

    ui.add_space(12.0);

    theme::card().show(ui, |ui| {
        ui.set_width(ui.available_width());
        theme::caption(ui, "AJAN (TRAY)");
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(
                "confsync-agent trayde durur, kaynakları düzenli denetler ve \
                 değişiklikte bildirim gönderir. Bu ayarlar ajan tarafından her \
                 turda yeniden okunur; yeniden başlatmak gerekmez.",
            )
            .size(12.5)
            .color(theme::MUTED),
        );
        ui.add_space(10.0);

        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Denetim aralığı").color(theme::MUTED));
            if ui
                .add(egui::DragValue::new(&mut app.settings.agent_interval_min).range(1..=180))
                .changed()
            {
                changed = true;
            }
            ui.label(egui::RichText::new("dakika").size(12.5).color(theme::FAINT));
        });

        ui.add_space(8.0);
        if ui
            .checkbox(
                &mut app.settings.agent_auto_backup,
                "Karar gerektirmeyen değişiklikleri kendiliğinden yedekle",
            )
            .on_hover_text(
                "Sır şüphesi ya da boyut sınırı nedeniyle karar bekleyen dosya varsa \
                 ajan yine de yedeklemez; yalnızca bildirim gönderir.",
            )
            .changed()
        {
            changed = true;
        }
    });

    ui.add_space(12.0);

    let remembered = app.settings.always_include.len() + app.settings.always_skip.len();
    if remembered > 0 {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                theme::caption(ui, &format!("HATIRLANAN KARARLAR ({remembered})"));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(theme::ghost_button("Hepsini unut")).clicked() {
                        app.settings.always_include.clear();
                        app.settings.always_skip.clear();
                        changed = true;
                        app.info("Hatırlanan kararlar silindi; bu dosyalar yeniden sorulacak.");
                    }
                });
            });
            ui.add_space(6.0);

            let home = settings::home_dir();
            let mut forget: Option<(PathBuf, bool)> = None;
            for (path, include) in app
                .settings
                .always_include
                .iter()
                .map(|p| (p, true))
                .chain(app.settings.always_skip.iter().map(|p| (p, false)))
            {
                theme::inset().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        theme::badge(
                            ui,
                            if include { "yedeğe girer" } else { "atlanır" },
                            if include { theme::SUCCESS } else { theme::MUTED },
                        );
                        ui.label(
                            egui::RichText::new(paths::display_short(path, &home))
                                .size(12.5)
                                .color(theme::TEXT),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.add(theme::ghost_button("Unut")).clicked() {
                                forget = Some((path.clone(), include));
                            }
                        });
                    });
                });
                ui.add_space(4.0);
            }

            if let Some((path, include)) = forget {
                if include {
                    app.settings.always_include.retain(|p| p != &path);
                } else {
                    app.settings.always_skip.retain(|p| p != &path);
                }
                changed = true;
            }
        });
    }

    if changed {
        app.settings_dirty = true;
    }

    ui.add_space(12.0);
    save_row(app, ui);
}

// --- ortak yardımcılar ---------------------------------------------------

fn save_row(app: &mut App, ui: &mut egui::Ui) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(app.settings_dirty, theme::primary_button("Ayarları kaydet"))
            .clicked()
        {
            app.save_settings();
        }
        if app.settings_dirty {
            ui.label(
                egui::RichText::new("kaydedilmemiş değişiklik var")
                    .size(12.5)
                    .color(theme::WARN),
            );
        }
    });
}

fn action_text(action: Action) -> egui::RichText {
    let color = match action {
        Action::Create => theme::SUCCESS,
        Action::Overwrite => theme::WARN,
        Action::Unchanged => theme::FAINT,
        Action::Conflict | Action::Missing => theme::DANGER,
    };
    egui::RichText::new(action.label()).size(12.5).color(color)
}

fn level_color(level: LogLevel) -> egui::Color32 {
    match level {
        LogLevel::Info => theme::MUTED,
        LogLevel::Warn => theme::WARN,
        LogLevel::Error => theme::DANGER,
    }
}

fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let tail: String = text
        .chars()
        .skip(text.chars().count().saturating_sub(max - 1))
        .collect();
    format!("…{tail}")
}

fn human_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    // Bayt için ondalık anlamsız: "904 B", "4.6 KiB".
    if unit == 0 {
        format!("{value:.0} {}", UNITS[unit])
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
