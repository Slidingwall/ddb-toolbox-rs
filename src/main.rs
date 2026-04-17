use iced::widget::{
    button, column, row, text, text_input, checkbox, scrollable,
};
use iced::{
    Alignment, Element, Length, Renderer, Size, Task, Theme, window
};
use rfd::{MessageDialog, FileDialog};
use std::path::{Path, PathBuf};
use std::thread;
use anyhow;
mod ddi;
mod extract_ddi;
mod extract_frm2;
mod extract_wav;
mod seg;
mod mixins_ddb;
mod pack_ddb;
#[derive(Debug, Clone, PartialEq, Default)]
pub enum MixinsMode {
    #[default]
    Vqm,
    Sta2Vqm,
}
#[derive(Debug, Clone)]
pub enum Message {
    BrowseDdi,
    BrowseDdb,
    BrowseOutDir,
    BrowseTree,
    BrowsePackOut,
    BrowseSrcDdi,
    BrowseMixDdi,
    BrowseMixOut,
    RunExtractWav,
    RunExtractDdi,
    RunExtractFrm2,
    RunPack,
    RunMixins,
    TaskCompleted(Result<(), String>),
    ToggleGenLab(bool),
    ToggleGenSeg(bool),
    ToggleClassify(bool),
    ToggleSaveTemp(bool),
    ToggleCatOnly(bool),
}
#[derive(Default)]
pub struct AppState {
    ddi_file: String,
    ddb_file: String,
    out_dir: String,
    tree_path: String,
    pack_out: String,
    src_ddi: String,
    mix_ddi: String,
    mix_out: String,
    gen_lab: bool,
    gen_seg: bool,
    classify: bool,
    save_temp: bool,
    cat_only: bool,
    mix_mode: MixinsMode,
    is_busy: bool,
}
impl AppState {
    fn update(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::ToggleGenLab(value) => {
                self.gen_lab = value;
                Task::none()
            }
            Message::ToggleGenSeg(value) => {
                self.gen_seg = value;
                Task::none()
            }
            Message::ToggleClassify(value) => {
                self.classify = value;
                Task::none()
            }
            Message::ToggleSaveTemp(value) => {
                self.save_temp = value;
                Task::none()
            }
            Message::ToggleCatOnly(value) => {
                self.cat_only = value;
                Task::none()
            }
            Message::BrowseDdi => {
                self.ddi_file = FileDialog::new()
                    .add_filter("ddi", &["ddi"])
                    .pick_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseDdb => {
                self.ddb_file = FileDialog::new()
                    .add_filter("ddb", &["ddb"])
                    .pick_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseOutDir => {
                self.out_dir = FileDialog::new()
                    .pick_folder()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseTree => {
                self.tree_path = FileDialog::new()
                    .add_filter("tree", &["tree"])
                    .pick_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowsePackOut => {
                self.pack_out = FileDialog::new()
                    .pick_folder()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseSrcDdi => {
                self.src_ddi = FileDialog::new()
                    .add_filter("ddi", &["ddi"])
                    .pick_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseMixDdi => {
                self.mix_ddi = FileDialog::new()
                    .add_filter("ddi", &["ddi"])
                    .pick_file()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::BrowseMixOut => {
                self.mix_out = FileDialog::new()
                    .pick_folder()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                Task::none()
            }
            Message::RunExtractWav => {
                if self.is_busy { return Task::none(); }
                self.is_busy = true;
                let ddi = self.ddi_file.clone();
                let out = self.out_dir.clone();
                let gl = self.gen_lab;
                let gs = self.gen_seg;
                let cl = self.classify;
                Task::perform(async move {
                    let res = thread::spawn(move || {
                        extract_wav::main(Path::new(&ddi), PathBuf::from(&out), gl, gs, cl)
                    }).join().unwrap_or_else(|_| Err(anyhow::anyhow!("panic")));
                    res.map_err(|e| e.to_string())
                }, Message::TaskCompleted)
            }
            Message::RunExtractDdi => {
                if self.is_busy { return Task::none(); }
                self.is_busy = true;
                let path = self.ddi_file.clone();
                let st = self.save_temp;
                let co = self.cat_only;
                Task::perform(async move {
                    let res = thread::spawn(move || {
                        extract_ddi::main(Path::new(&path), st, co)
                    }).join().unwrap_or_else(|_| Err(anyhow::anyhow!("panic")));
                    res.map_err(|e| e.to_string())
                }, Message::TaskCompleted)
            }
            Message::RunExtractFrm2 => {
                if self.is_busy { return Task::none(); }
                self.is_busy = true;
                let ddb = self.ddb_file.clone();
                let out = self.out_dir.clone();
                Task::perform(async move {
                    let zip = PathBuf::from(&out).join("frm2.zip");
                    let res = thread::spawn(move || {
                        extract_frm2::main(Path::new(&ddb), Some(&zip))
                    }).join().unwrap_or_else(|_| Err(anyhow::anyhow!("panic")));
                    res.map_err(|e| e.to_string())
                }, Message::TaskCompleted)
            }
            Message::RunPack => {
                if self.is_busy { return Task::none(); }
                self.is_busy = true;
                let tree = self.tree_path.clone();
                let out = self.pack_out.clone();
                Task::perform(async move {
                    let out_path = (!out.is_empty()).then(|| PathBuf::from(&out));
                    let res = thread::spawn(move || {
                        pack_ddb::main(Path::new(&tree), out_path.as_deref())
                    }).join().unwrap_or_else(|_| Err(anyhow::anyhow!("panic")));
                    res.map_err(|e| e.to_string())
                }, Message::TaskCompleted)
            }
            Message::RunMixins => {
                if self.is_busy { return Task::none(); }
                self.is_busy = true;
                let src = self.src_ddi.clone();
                let mix = self.mix_ddi.clone();
                let out = self.mix_out.clone();
                let mode = match self.mix_mode {
                    MixinsMode::Vqm => "vqm",
                    MixinsMode::Sta2Vqm => "sta2vqm",
                };
                Task::perform(async move {
                    let out_path = (!out.is_empty()).then(|| PathBuf::from(&out));
                    let res = thread::spawn(move || {
                        mixins_ddb::main(Path::new(&src), Some(Path::new(&mix)), out_path.as_deref(), mode, "Grw")
                    }).join().unwrap_or_else(|_| Err(anyhow::anyhow!("panic")));
                    res.map_err(|e| e.to_string())
                }, Message::TaskCompleted)
            }
            Message::TaskCompleted(res) => {
                self.is_busy = false;
                let _ = match res {
                    Ok(_) => MessageDialog::new().set_title("Success").set_description("Completed!"),
                    Err(e) => MessageDialog::new().set_title("Error").set_description(&e),
                }.show();
                Task::none()
            }
        }
    }
    fn view<'a>(&'a self) -> Element<'a, Message, Theme, Renderer> {
        let content = column![
            text("DDB Toolbox").size(24),
            text("Extract Tools").size(18),
            row![
                text("DDI File:"),
                text_input("Select DDI file", &self.ddi_file),
                button("Browse").on_press(Message::BrowseDdi),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("DDB File:"),
                text_input("Select DDB file", &self.ddb_file),
                button("Browse").on_press(Message::BrowseDdb),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("Output:"),
                text_input("Select output directory", &self.out_dir),
                button("Browse").on_press(Message::BrowseOutDir),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("WAV Options:").size(16),
                checkbox(self.gen_lab).on_toggle(Message::ToggleGenLab).label("Generate .lab"),
                checkbox(self.gen_seg).on_toggle(Message::ToggleGenSeg).label("Generate .seg"),
                checkbox(self.classify).on_toggle(Message::ToggleClassify).label("Classify"),
            ].spacing(8),
            row![
                text("DDI Options:").size(16),
                checkbox(self.save_temp).on_toggle(Message::ToggleSaveTemp).label("Save temp"),
                checkbox(self.cat_only).on_toggle(Message::ToggleCatOnly).label("Cat only"),
            ].spacing(8),
            row![
                button("Extract WAV").on_press(Message::RunExtractWav),
                button("Extract DDI").on_press(Message::RunExtractDdi),
                button("Extract FRM2").on_press(Message::RunExtractFrm2),
            ].spacing(8),
            text("Pack").size(18),
            row![
                text(".tree File:"),
                text_input("Select .tree file", &self.tree_path),
                button("Browse").on_press(Message::BrowseTree),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("Output:"),
                text_input("Select pack output directory", &self.pack_out),
                button("Browse").on_press(Message::BrowsePackOut),
            ].spacing(8).align_y(Alignment::Center),
            button("Pack").on_press(Message::RunPack),
            text("Mixins").size(18),
            row![
                text("Source DDI:"),
                text_input("Select source DDI", &self.src_ddi),
                button("Browse").on_press(Message::BrowseSrcDdi),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("Mix DDI:"),
                text_input("Select mix DDI", &self.mix_ddi),
                button("Browse").on_press(Message::BrowseMixDdi),
            ].spacing(8).align_y(Alignment::Center),
            row![
                text("Output:"),
                text_input("Select mix output directory", &self.mix_out),
                button("Browse").on_press(Message::BrowseMixOut),
            ].spacing(8).align_y(Alignment::Center),
            button("Run Mixins").on_press(Message::RunMixins),
        ]
        .spacing(10)
        .padding(20)
        .width(Length::Fill);
        scrollable(content).into()
    }
}
fn app_view(state: &AppState) -> Element<'_, Message, Theme, Renderer> {
    state.view()
}
fn main() -> iced::Result {
    iced::application(
        || (AppState::default(), Task::none()),
        |state: &mut AppState, message| state.update(message),
        app_view, 
    ).window(window::Settings {
        size: Size::new(650.0, 700.0),
        resizable: false,
        ..window::Settings::default()
    }).run()
}