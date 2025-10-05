use core::f32;
use dicom::object::open_file;
use dicom_dump::DumpOptions;
use egui::Widget;
use egui_ltreeview::{Action, TreeView};
use regex::RegexBuilder;
use rsdirtreebuilder::dir_tree_builder::{DirTreeBuilder, PathSizeInfo};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    time::Duration,
};

/// We derive Deserialize/Serialize so we can persist app state on shutdown.
pub struct TemplateApp {
    base_dir: PathBuf,
    dicom_files: Vec<PathSizeInfo>,
    selected_file: Option<PathBuf>,
    search_input: String,
    dicom_dump: HashMap<PathBuf, String>,
    matched_pos: Option<usize>,
    scroll_pos: Option<usize>,
    search_results: Option<Vec<usize>>,
}

impl TemplateApp {
    /// Called once before the first frame.
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // This is also where you can customize the look and feel of egui using
        // `cc.egui_ctx.set_visuals` and `cc.egui_ctx.set_fonts`.

        // Load previous app state (if any).
        // Note that you must enable the `persistence` feature for this to work.
        cc.egui_ctx.set_theme(egui::Theme::Light);
        cc.egui_ctx.set_pixels_per_point(1.2);

        Self {
            base_dir: PathBuf::new(),
            dicom_files: Vec::new(),
            selected_file: None,
            search_input: "".to_string(),
            dicom_dump: HashMap::new(),
            matched_pos: None,
            scroll_pos: None,
            search_results: None,
        }
    }
}

impl TemplateApp {
    /// Handle dir open by enumerating the directory recursively and storing the dicom files.
    fn handle_file_open(&mut self, path: &Path) {
        self.dicom_files.clear();
        self.base_dir = path.to_path_buf();

        let builder = DirTreeBuilder::build(rsdirtreebuilder::dir_tree_builder::Params::new(path))
            .expect("Builder expected to be created");

        while !builder.is_finished() {
            std::thread::sleep(Duration::from_millis(100));
        }

        self.dicom_files = builder
            .iter()
            .filter(|x| !x.is_dir && open_file(&x.path).is_ok())
            .collect();

        self.dicom_files
            .sort_by(|a, b| a.path.as_os_str().cmp(b.path.as_os_str()));

        self.dicom_dump.clear();
    }

    /// Get the current selected dicom dump.
    fn get_dicom_dump(&self) -> &str {
        let entry = if let Some(selected_file) = self.selected_file.as_ref() {
            self.dicom_dump.get(selected_file)
        } else {
            None
        };

        if let Some(entry) = entry {
            entry.as_str()
        } else {
            ""
        }
    }

    /// Handle the search in the dicom dump.
    fn handle_search(&mut self, is_forward_search: bool) {
        // Search the dicom dump if there are no results yet.
        if self.search_results.is_none() {
            self.search_results = Some(self.search());
        }

        // Go and fetch the next match.
        if let Some(pos) = self.get_next_match(is_forward_search) {
            self.matched_pos = Some(pos);
            self.scroll_pos = Some(pos);
        } else {
            self.matched_pos = None;
            self.search_results = Some(Vec::new());
        }
    }

    /// Handle the dicom file selected by caching the entry.
    fn handle_file_selected(&mut self, node_id: &Path) {
        // Reset the currnent search results.
        self.selected_file = Some(node_id.to_path_buf());
        self.search_results = None;
        self.matched_pos = None;
        self.scroll_pos = Some(0);

        // Get the dicom dump from the cache or get it from the file.
        self.dicom_dump
            .entry(node_id.to_path_buf())
            .or_insert_with(|| {
                if let Ok(obj) = open_file(node_id) {
                    let mut out = Vec::new();

                    DumpOptions::new()
                        .width(256)
                        .color_mode(dicom_dump::ColorMode::Never)
                        .no_limit(false)
                        .no_text_limit(false)
                        .format(dicom_dump::DumpFormat::Text)
                        .dump_file_to_with_limits(&mut out, &obj)
                        .unwrap();
                    String::from_utf8(out).unwrap()
                } else {
                    "".to_string()
                }
            });
    }

    /// Search the dicom dump for the text.
    fn search(&self) -> Vec<usize> {
        let text = self.get_dicom_dump();
        let mut results = Vec::new();

        if text.is_empty() {
            return results;
        }

        let regex = RegexBuilder::new(&regex::escape(&self.search_input))
            .case_insensitive(true)
            .build()
            .unwrap();

        for (index, line) in text.split("\n").enumerate() {
            if regex.is_match(line) {
                results.push(index);
            }
        }

        results
    }

    /// Get the next match from the existing results.
    fn get_next_match(&self, forward_search: bool) -> Option<usize> {
        let search_results = self
            .search_results
            .as_ref()
            .expect("search results should be filled in");
        let matched_pos = self.matched_pos;

        if forward_search {
            if matched_pos.is_none_or(|p| search_results.last().is_some_and(|x| p >= *x)) {
                return search_results.first().copied();
            }
            search_results
                .iter()
                .find(|x| **x > matched_pos.expect("none check should be done before"))
                .copied()
        } else {
            if matched_pos.is_none_or(|p| search_results.first().is_some_and(|x| p <= *x)) {
                return search_results.last().copied();
            }
            search_results
                .iter()
                .rev()
                .find(|x| **x < matched_pos.expect("none check should be done before"))
                .copied()
        }
    }

    /// Build the UI treeview.
    /// The dicom files are assumed to be sorted by the paths, so that we will just need to follow the directory up and down.
    fn build_ui_treeview(&self, builder: &mut egui_ltreeview::TreeViewBuilder<'_, PathBuf>) {
        builder.dir(self.base_dir.clone(), self.base_dir.display().to_string());
        let mut current_dir = self.base_dir.to_path_buf();

        for entry in &self.dicom_files {
            let parent_dir = entry.path.parent().expect("a file should have a parent");

            // Go up until the directory of the file is under the current_dir.
            while !parent_dir.starts_with(&current_dir) {
                current_dir = current_dir
                    .parent()
                    .expect("a parent of current_dir should be a parent of the file")
                    .to_path_buf();
                builder.close_dir();
            }

            // Go down until the current_dir is the directory of the file.
            if let Ok(path_diff) = parent_dir.strip_prefix(&current_dir) {
                for p in path_diff.components() {
                    current_dir.push(p);
                    builder.dir(current_dir.clone(), p.as_os_str().display().to_string());
                }
            }

            // Now add the file.
            builder.leaf(
                entry.path.clone(),
                entry
                    .path
                    .file_name()
                    .expect("file entry should have a filename")
                    .display()
                    .to_string(),
            );
        }

        // Go up until the current_dir hits the base_dir.
        if let Ok(path_diff) = current_dir.strip_prefix(&self.base_dir) {
            for _ in path_diff.components() {
                builder.close_dir();
            }
        }

        // Close the base dir.
        builder.close_dir();
    }
}

impl eframe::App for TemplateApp {
    /// Called each time the UI needs repainting, which may be many times per second.
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Put your widgets into a `SidePanel`, `TopBottomPanel`, `CentralPanel`, `Window` or `Area`.
        // For inspiration and more examples, go to https://emilk.github.io/egui

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            // The top panel is often a good place for a menu bar:
            egui::MenuBar::new().ui(ui, |ui| {
                // NOTE: no File->Quit on web pages!
                let is_web = cfg!(target_arch = "wasm32");
                if !is_web {
                    ui.menu_button("File", |ui| {
                        if ui.button("Open").clicked()
                            && let Some(path) = rfd::FileDialog::new().pick_folder()
                        {
                            self.handle_file_open(&path);
                        }
                        if ui.button("Quit").clicked() {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                        }
                    });
                }
            });

            ui.horizontal(|ui| {
                if ui.button("📂").clicked()
                    && let Some(path) = rfd::FileDialog::new().pick_folder()
                {
                    self.handle_file_open(&path);
                }
            });
        });

        if !self.dicom_files.is_empty() {
            egui::SidePanel::left(egui::Id::new("tree view"))
                .resizable(true)
                .show(ctx, |ui| {
                    egui::ScrollArea::both()
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        )
                        .show(ui, |ui| {
                            let id = ui.make_persistent_id("Names tree view");
                            let (_response, actions) = TreeView::new(id)
                                // .override_indent(Some(2.0))
                                .show(ui, |builder| {
                                    self.build_ui_treeview(builder);
                                });

                            for action in actions.iter() {
                                match action {
                                    Action::SetSelected(nodes) => {
                                        nodes.iter().for_each(|node_id| {
                                            if node_id.is_file() {
                                                self.handle_file_selected(node_id);
                                            }
                                        });
                                    }
                                    _ => {
                                        // Not used.
                                    }
                                }
                            }
                        });
                });

            egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Search:");
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut self.search_input)
                            .desired_width(f32::INFINITY),
                    );
                    if response.changed() {
                        self.matched_pos = None;
                        self.search_results = None;
                    }
                    if response.lost_focus() && ui.input(|x| x.key_pressed(egui::Key::Enter)) {
                        self.handle_search(true);
                        response.request_focus();
                    }
                });

                ui.horizontal(|ui| {
                    if ui.button("Prev").clicked() {
                        self.handle_search(false);
                    }
                    if ui.button("Next").clicked() && !self.search_input.is_empty() {
                        self.handle_search(true);
                    }
                    if ui.button("Clear").clicked() {
                        self.search_input.clear();
                        self.matched_pos = None;
                        self.search_results = None;
                    }

                    let search_status = self
                        .search_results
                        .as_ref()
                        .map_or("".into(), |x| format!("{} matches", x.len()));

                    ui.label(egui::RichText::new(search_status).color(egui::Color32::BLUE));
                });

                ui.separator();

                egui::ScrollArea::both().show(ui, |ui| {
                    for (index, line) in self.get_dicom_dump().split("\n").enumerate() {
                        let rich_text = egui::RichText::new(line).monospace();

                        let response = if self.matched_pos.is_some_and(|x| x == index) {
                            egui::Label::new(
                                rich_text
                                    .strong()
                                    .background_color(egui::Color32::LIGHT_GRAY),
                            )
                            .wrap_mode(egui::TextWrapMode::Extend)
                            .ui(ui)
                        } else {
                            egui::Label::new(rich_text).ui(ui)
                        };

                        if self.scroll_pos.is_some_and(|x| x == index) {
                            response.scroll_to_me(None);
                        }
                    }
                    self.scroll_pos = None;
                });
            });
        } else {
            egui::CentralPanel::default().show(ctx, |_ui| {});
        }
    }
}
