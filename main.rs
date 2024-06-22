use async_std::fs;
use async_std::prelude::StreamExt;
use eframe::{egui, NativeOptions};
use egui::{ProgressBar, Ui};
use filetime_creation::{set_file_ctime, set_file_mtime, FileTime};
use rfd::FileDialog;
use serde_json::Value;
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use std::vec::Vec;

// todo
// error window
// show skipped files count
// show matched files count
// geo data
// subdirs
// optionally delete jsons

fn main() -> Result<(), eframe::Error> {
    env_logger::init();
    let options = NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([360.0, 240.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Google Photos metadata matcher",
        options,
        Box::new(|_cc| Box::<Matcher>::default()),
    )
}

struct Matcher {
    folder_path: String,
    selected_folder: Option<PathBuf>,
    should_copy: bool,
    should_go_over_subdirs: bool,
    progress: Arc<Mutex<f32>>,
    working_message: String,
}

impl Default for Matcher {
    fn default() -> Self {
        Self {
            folder_path: String::new(),
            selected_folder: None,
            should_copy: false,
            should_go_over_subdirs: false,
            progress: Arc::new(Mutex::new(0.0)),
            working_message: "".to_string(),
        }
    }
}

impl eframe::App for Matcher {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui: &mut Ui| {
            ui.horizontal(|ui: &mut Ui| {
                // Select folder by entering path
                ui.label("Folder Path:");
                ui.add(egui::TextEdit::singleline(&mut self.folder_path).desired_width(200.0));
                if ui.button("Select").clicked() {
                    if let Ok(path) = std::fs::canonicalize(&self.folder_path) {
                        self.selected_folder = Some(path);
                    } else {
                        self.selected_folder = None;
                    }
                }
            });

            // Select folder button by window dialog
            if ui.button("Select Folder").clicked() {
                if let Some(folder) = FileDialog::new()
                    .set_directory(&std::env::current_dir().unwrap())
                    .pick_folder()
                {
                    self.selected_folder = Some(folder);
                }
            }

            // Show selected folder in ui
            if let Some(folder) = self.selected_folder.as_ref().and_then(|p| p.to_str()) {
                ui.label(format!("Selected folder: {}", folder));
            } else {
                ui.label("No folder selected");
            }

            // Those are not implemented
            // ui.checkbox(&mut self.should_copy, "Copy photos to new folder");
            // ui.checkbox(&mut self.should_go_over_subdirs, "Go over subdirectories");
            // ui.checkbox(, "Delete json files");

            // Match metadata button
            if ui.button("Match metadata").clicked() {
                if self.selected_folder == None {
                    return;
                }

                self.working_message = "Working...".to_string();

                let matcher = Matcher {
                    folder_path: self.folder_path.clone(),
                    selected_folder: self.selected_folder.clone(),
                    should_copy: self.should_copy,
                    should_go_over_subdirs: self.should_go_over_subdirs,
                    progress: self.progress.clone(),
                    working_message: "".to_string(),
                };

                let ctx_clone = ctx.clone();

                async_std::task::spawn(async move {
                    match matcher.selected_folder {
                        Some(folder) => {
                            match_metadata(
                                folder,
                                matcher.should_go_over_subdirs,
                                matcher.should_copy,
                                matcher.progress,
                                ctx_clone,
                            )
                            .await;
                        }
                        None => {
                            println!("No folder selected");
                        }
                    }
                });
            }

            // Progress bar
            let prog = self.progress.lock().unwrap();
            if *prog > 0.0 && *prog < 1.0 {
                ui.add(ProgressBar::new(*prog).show_percentage());
            } else if *prog >= 1.0 {
                ui.label("Metadata processing complete");
                self.working_message = "".to_string();
            }

            ui.label(&self.working_message);
        });
    }
}

async fn match_metadata(
    path: PathBuf,
    search_subdirs: bool,
    copy: bool,
    progress: Arc<Mutex<f32>>,
    ctx: egui::Context,
) {
    println!("copy photos: {}", copy);
    println!("subdirs: {}", search_subdirs);
    println!("path: {:?}", path);

    let (progress_sender, progress_receiver) = mpsc::channel();

    // let ctx_clone = ctx.clone();

    async_std::task::spawn(async move {
        if search_subdirs {
            unimplemented!();
            // disappears immediately due to request_repaint() on progress_receiver.recv()
            // display_error(
            //     &ctx_clone,
            //     "Searching subdirectories is currently unimplemented.",
            // )
            // .await;
            // return;
        } else {
            // search for jsons
            let json_paths = get_jsons(&path).await;
            let metadata = extract_metadata(json_paths).await;

            // open the files by the title inside of the json file and match the timestamps to the images
            match metadata {
                Ok(m) => {
                    let total_elements = m.len();
                    let mut current_element = 0;

                    for element in m {
                        open_and_match(element, &path);

                        // progress
                        current_element += 1;
                        let progress = (current_element as f32) / (total_elements as f32);

                        match progress_sender.send(progress) {
                            Ok(_) => println!("Sent progress: {}", progress),
                            Err(err) => println!("Error sending progress: {}", err),
                        }
                    }
                }
                Err(e) => {
                    println!("Error: {}", e);
                    // display_error(&ctx_clone, e.as_str()).await;
                    return;
                }
            }

            progress_sender.send(1.0).unwrap();
        }
    });

    while let Ok(p) = progress_receiver.recv() {
        ctx.request_repaint();
        *progress.lock().unwrap() = p;
    }
}

async fn get_jsons(path: &PathBuf) -> Vec<async_std::path::PathBuf> {
    let mut json_paths = Vec::new();

    if let Ok(mut entries) = fs::read_dir(&path).await {
        while let Some(entry) = entries.next().await {
            if let Ok(entry) = entry {
                let file_path = entry.path();
                if let Some(extension) = file_path.extension() {
                    if extension == "json" {
                        json_paths.push(file_path);
                    }
                }
            }
        }
    } else {
        println!("Failed to read directory: {:?}", path);
    }

    json_paths
}

struct GPhotosMetadata {
    title: String,
    phototaken_timestamp: i64,
    // todo geo data
}

async fn extract_metadata(
    json_paths: Vec<async_std::path::PathBuf>,
) -> Result<Vec<GPhotosMetadata>, String> {
    let mut all_files_metadata = Vec::new();

    for json_path in json_paths {
        let file_content = async_std::fs::read_to_string(&json_path)
            .await
            .map_err(|err| format!("Failed to read JSON file {:?}: {}", json_path, err))?;

        let json_value = serde_json::from_str::<Value>(&file_content)
            .map_err(|err| format!("Failed to parse JSON file {:?}: {}", json_path, err))?;

        let title = if let Some(t) = json_value.get("title") {
            t.as_str().unwrap().to_string()
        } else {
            return Err(format!(
                "JSON file {:?} does not contain 'title' property",
                json_path
            ));
        };

        let cr_time = if let Some(creation_time) = json_value.get("photoTakenTime") {
            let timestamp = creation_time["timestamp"].as_str().unwrap();
            timestamp.parse::<i64>().unwrap()
        } else {
            return Err(format!(
                "JSON file {:?} does not contain 'photoTakenTime' property",
                json_path
            ));
        };

        let metadata = GPhotosMetadata {
            title: title,
            phototaken_timestamp: cr_time,
        };

        all_files_metadata.push(metadata);
    }

    return Ok(all_files_metadata);
}

// async fn display_error(ctx: &egui::Context, message: &str) {
//     egui::Window::new("Error").show(ctx, |ui| {
//         ui.add(egui::Label::new(message));
//     });
// }

fn open_and_match(el: GPhotosMetadata, path: &PathBuf) {
    println!("{:?}", el.title);
    println!("{:?}", el.phototaken_timestamp);

    // Photo taken time
    let phototaken_time = SystemTime::UNIX_EPOCH + Duration::new(el.phototaken_timestamp as u64, 0);
    FileTime::from_system_time(phototaken_time);
    let phototaken_filetime = FileTime::from_system_time(phototaken_time);

    // todo geo data

    let file_path = path.join(&el.title);

    if !file_path.exists() {
        println!("File {:?} does not exist, skipping...", file_path);
        return;
    }

    set_file_ctime(&file_path, phototaken_filetime).expect("Failed to set creation file time");
    set_file_mtime(&file_path, phototaken_filetime).expect("Failed to set modification file time");
}
