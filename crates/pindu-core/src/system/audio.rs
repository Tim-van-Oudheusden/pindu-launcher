//! The system panel's Sound Output section (`docs/system-panel.md`, "Audio output"): the
//! outputs PipeWire has and the one in use, read from `pw-dump`'s JSON, and the choice of
//! another one, which `wpctl set-default` makes and WirePlumber keeps across restarts.
//! PipeWire and WirePlumber have no D-Bus interface for this, so the backend runs the two
//! tools; in the Flatpak, which has neither, the section is hidden.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::{Rc, Weak};

use serde_json::Value;

/// The programs the section runs: `pw-dump` reads the outputs, `wpctl` sets the default.
/// Without either the section is hidden.
pub const TOOLS: [&str; 2] = ["pw-dump", "wpctl"];

/// The command that dumps PipeWire's objects as JSON, once.
pub const DUMP_COMMAND: [&str; 1] = ["pw-dump"];

/// The command that makes the output with PipeWire object id `id` the default: an argv
/// list, never a shell string.
pub fn set_default_command(id: u32) -> [String; 3] {
    ["wpctl".to_owned(), "set-default".to_owned(), id.to_string()]
}

/// An audio output: a PipeWire node of class `Audio/Sink`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The node's object id, which `wpctl set-default` takes.
    pub id: u32,
    /// The node's `node.name`, which the `default` metadata names.
    pub name: String,
    /// What the output is called: its `node.description`, such as "WH-1000XM4" or "HDA
    /// NVidia Digital Stereo (HDMI)", or its name when it has none.
    pub description: String,
}

/// The outputs in a dump, in PipeWire's order, and the default one.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Outputs {
    /// Every `Audio/Sink` node.
    pub outputs: Vec<Output>,
    /// The id of the output `default.audio.sink` names, when it names one of them.
    pub default: Option<u32>,
}

/// Why a dump could not be read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DumpError {
    /// The text is not JSON.
    NotJson(String),
    /// The JSON is not a list of PipeWire objects.
    NotADump,
}

impl fmt::Display for DumpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotJson(error) => write!(f, "pw-dump's output is not JSON: {error}"),
            Self::NotADump => f.write_str("pw-dump's output is not a list of objects"),
        }
    }
}

impl std::error::Error for DumpError {}

impl Outputs {
    /// Reads the outputs and the default one from `pw-dump`'s JSON: a list of PipeWire
    /// objects, of which the `Audio/Sink` nodes are the outputs and the `default` metadata
    /// names the default one by its `node.name`.
    pub fn parse(json: &str) -> Result<Self, DumpError> {
        let dump: Value =
            serde_json::from_str(json).map_err(|error| DumpError::NotJson(error.to_string()))?;
        let objects = dump.as_array().ok_or(DumpError::NotADump)?;
        let outputs: Vec<Output> = objects.iter().filter_map(output).collect();
        let default = objects.iter().find_map(default_sink).and_then(|name| {
            outputs
                .iter()
                .find(|output| output.name == name)
                .map(|output| output.id)
        });
        Ok(Self { outputs, default })
    }

    /// The output in use: the default one.
    pub fn in_use(&self) -> Option<&Output> {
        let default = self.default?;
        self.outputs.iter().find(|output| output.id == default)
    }
}

/// The output `object` is, when it is an `Audio/Sink` node.
fn output(object: &Value) -> Option<Output> {
    if object["type"] != "PipeWire:Interface:Node" {
        return None;
    }
    let props = &object["info"]["props"];
    if props["media.class"] != "Audio/Sink" {
        return None;
    }
    let id = u32::try_from(object["id"].as_u64()?).ok()?;
    let name = props["node.name"].as_str()?.to_owned();
    let description = props["node.description"]
        .as_str()
        .filter(|description| !description.is_empty())
        .map_or_else(|| name.clone(), str::to_owned);
    Some(Output {
        id,
        name,
        description,
    })
}

/// The `node.name` of the default sink, when `object` is the `default` metadata and names
/// one.
fn default_sink(object: &Value) -> Option<&str> {
    if object["type"] != "PipeWire:Interface:Metadata"
        || object["props"]["metadata.name"] != "default"
    {
        return None;
    }
    object["metadata"].as_array()?.iter().find_map(|entry| {
        (entry["subject"] == 0 && entry["key"] == "default.audio.sink")
            .then(|| entry["value"]["name"].as_str())
            .flatten()
    })
}

/// What the backend found when it read the outputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dump {
    /// The tools cannot run here: the Flatpak sandbox, or [`TOOLS`] not installed.
    Unavailable,
    /// `pw-dump`'s output.
    Text(String),
    /// `pw-dump` did not run or failed, for example with PipeWire not running.
    Failed(String),
}

/// What a backend reports once an action ends: `Err` says why it failed.
pub type Done = Box<dyn FnOnce(Result<(), String>)>;

/// What a backend reports once a read ends.
pub type ReadDone = Box<dyn FnOnce(Dump)>;

/// Runs the tools for the Sound Output section. Every callback runs on the main thread.
pub trait AudioBackend {
    /// Runs [`DUMP_COMMAND`] and reports what it found to `done`.
    fn read(&self, done: ReadDone);

    /// Runs [`set_default_command`] for `id` and reports to `done` once it has ended.
    fn set_default(&self, id: u32, done: Done);
}

/// The section as the shells show it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AudioState {
    /// Whether Quick Access shows the section: the tools ran and found an output.
    pub available: bool,
    /// The outputs and the default one.
    pub outputs: Outputs,
}

/// Something that went wrong, for the shells to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The outputs could not be read; the section is hidden.
    Read(String),
    /// The chosen output did not become the default.
    Choose(String),
}

/// Something a shell does when the state changes.
type Listener = Box<dyn Fn(&AudioState)>;
/// Something a shell does when something fails.
type FailureListener = Box<dyn Fn(&Failure)>;

/// The Sound Output section over its backend: reads the outputs, and makes the chosen one
/// the default.
#[derive(Clone)]
pub struct SoundOutput {
    inner: Rc<Inner>,
}

struct Inner {
    backend: Box<dyn AudioBackend>,
    state: RefCell<AudioState>,
    /// Counts the choices made, so a read that started before one does not undo it.
    choices: Cell<u64>,
    listeners: RefCell<Vec<Listener>>,
    failure_listeners: RefCell<Vec<FailureListener>>,
}

impl fmt::Debug for SoundOutput {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SoundOutput")
            .field("state", &self.inner.state.borrow())
            .finish_non_exhaustive()
    }
}

impl SoundOutput {
    /// The section over `backend`, hidden until the first [`refresh`](Self::refresh) reads
    /// it, so the shells can connect to it first.
    pub fn new(backend: Box<dyn AudioBackend>) -> Self {
        Self {
            inner: Rc::new(Inner {
                backend,
                state: RefCell::default(),
                choices: Cell::new(0),
                listeners: RefCell::default(),
                failure_listeners: RefCell::default(),
            }),
        }
    }

    /// The state as last read.
    pub fn state(&self) -> AudioState {
        self.inner.state.borrow().clone()
    }

    /// Calls `listener` whenever the state changes.
    pub fn connect_changed(&self, listener: impl Fn(&AudioState) + 'static) {
        self.inner.listeners.borrow_mut().push(Box::new(listener));
    }

    /// Calls `listener` whenever something goes wrong.
    pub fn connect_failed(&self, listener: impl Fn(&Failure) + 'static) {
        self.inner
            .failure_listeners
            .borrow_mut()
            .push(Box::new(listener));
    }

    /// Reads the outputs again: Quick Access or the level opens.
    pub fn refresh(&self) {
        let choices = self.inner.choices.get();
        let weak = self.downgrade();
        self.inner.backend.read(Box::new(move |dump| {
            if let Some(output) = upgrade(&weak)
                && output.inner.choices.get() == choices
            {
                output.take(dump);
            }
        }));
    }

    /// Makes the output with id `id` the default. The level checks it at once; a choice
    /// that fails is reported, and the outputs are read again to show the default as it is.
    pub fn choose(&self, id: u32) {
        {
            let mut state = self.inner.state.borrow_mut();
            let known = state.outputs.outputs.iter().any(|output| output.id == id);
            if !state.available || !known || state.outputs.default == Some(id) {
                return;
            }
            state.outputs.default = Some(id);
        }
        self.inner.choices.set(self.inner.choices.get() + 1);
        self.changed();
        let weak = self.downgrade();
        self.inner.backend.set_default(
            id,
            Box::new(move |result| {
                if let (Some(output), Err(error)) = (upgrade(&weak), result) {
                    output.fail(&Failure::Choose(error));
                    output.refresh();
                }
            }),
        );
    }

    /// Takes what the backend read: the section shows while the tools ran and found an
    /// output. A dump that could not be read is reported and hides it.
    fn take(&self, dump: Dump) {
        let outputs = match dump {
            Dump::Unavailable => Outputs::default(),
            Dump::Text(text) => Outputs::parse(&text).unwrap_or_else(|error| {
                self.fail(&Failure::Read(error.to_string()));
                Outputs::default()
            }),
            Dump::Failed(error) => {
                self.fail(&Failure::Read(error));
                Outputs::default()
            }
        };
        let read = AudioState {
            available: !outputs.outputs.is_empty(),
            outputs,
        };
        if self.inner.state.replace(read.clone()) != read {
            self.changed();
        }
    }

    fn changed(&self) {
        let state = self.state();
        for listener in self.inner.listeners.borrow().iter() {
            listener(&state);
        }
    }

    fn fail(&self, failure: &Failure) {
        for listener in self.inner.failure_listeners.borrow().iter() {
            listener(failure);
        }
    }

    fn downgrade(&self) -> Weak<Inner> {
        Rc::downgrade(&self.inner)
    }
}

/// The section again from a backend's callback, unless it is gone.
fn upgrade(weak: &Weak<Inner>) -> Option<SoundOutput> {
    weak.upgrade().map(|inner| SoundOutput { inner })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn fixture(name: &str) -> String {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pw_dump")
            .join(format!("{name}.json"));
        std::fs::read_to_string(&path).unwrap_or_else(|error| panic!("{path:?}: {error}"))
    }

    fn outputs(name: &str) -> Outputs {
        Outputs::parse(&fixture(name)).unwrap()
    }

    fn descriptions(outputs: &Outputs) -> Vec<(u32, &str)> {
        outputs
            .outputs
            .iter()
            .map(|output| (output.id, output.description.as_str()))
            .collect()
    }

    #[test]
    fn one_output_is_listed_and_in_use() {
        let outputs = outputs("one-output");

        assert_eq!(
            descriptions(&outputs),
            [(56, "Built-in Audio Analog Stereo")]
        );
        assert_eq!(outputs.default, Some(56));
        let in_use = outputs.in_use().unwrap();
        assert_eq!(in_use.name, "alsa_output.pci-0000_00_1f.3.analog-stereo");
    }

    #[test]
    fn several_outputs_are_listed_without_inputs_or_streams() {
        let outputs = outputs("several-outputs");

        assert_eq!(
            descriptions(&outputs),
            [
                (56, "Built-in Audio Analog Stereo"),
                (61, "HDA NVidia Digital Stereo (HDMI)"),
                (63, "Yamaha AV Receiver Digital Stereo (IEC958)"),
            ]
        );
    }

    #[test]
    fn the_default_is_the_sink_in_use_not_the_one_configured() {
        let outputs = outputs("several-outputs");

        assert_eq!(outputs.default, Some(61));
        assert_eq!(
            outputs.in_use().unwrap().description,
            "HDA NVidia Digital Stereo (HDMI)"
        );
    }

    #[test]
    fn a_bluetooth_headset_is_an_output_and_can_be_the_default() {
        let outputs = outputs("bluetooth-headset");

        assert_eq!(
            descriptions(&outputs),
            [(56, "Built-in Audio Analog Stereo"), (74, "WH-1000XM4")]
        );
        assert_eq!(outputs.in_use().unwrap().id, 74);
    }

    #[test]
    fn without_a_default_the_outputs_are_listed_and_none_is_in_use() {
        let outputs = outputs("no-default");

        assert_eq!(outputs.outputs.len(), 2);
        assert_eq!(outputs.default, None);
        assert_eq!(outputs.in_use(), None);
    }

    #[test]
    fn no_default_metadata_at_all_means_no_default() {
        let json = r#"[{"id": 56, "type": "PipeWire:Interface:Node", "info": {"props":
            {"media.class": "Audio/Sink", "node.name": "a", "node.description": "A"}}}]"#;

        let outputs = Outputs::parse(json).unwrap();

        assert_eq!(descriptions(&outputs), [(56, "A")]);
        assert_eq!(outputs.default, None);
    }

    #[test]
    fn a_default_naming_no_output_is_no_default() {
        let json = r#"[
            {"id": 56, "type": "PipeWire:Interface:Node", "info": {"props":
                {"media.class": "Audio/Sink", "node.name": "a", "node.description": "A"}}},
            {"id": 40, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "default"},
                "metadata": [{"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON",
                    "value": {"name": "gone"}}]}
        ]"#;

        assert_eq!(Outputs::parse(json).unwrap().default, None);
    }

    #[test]
    fn another_metadata_object_does_not_name_the_default() {
        let json = r#"[
            {"id": 56, "type": "PipeWire:Interface:Node", "info": {"props":
                {"media.class": "Audio/Sink", "node.name": "a", "node.description": "A"}}},
            {"id": 41, "type": "PipeWire:Interface:Metadata", "props": {"metadata.name": "other"},
                "metadata": [{"subject": 0, "key": "default.audio.sink", "type": "Spa:String:JSON",
                    "value": {"name": "a"}}]}
        ]"#;

        assert_eq!(Outputs::parse(json).unwrap().default, None);
    }

    #[test]
    fn an_output_without_a_description_goes_by_its_name() {
        let json = r#"[{"id": 56, "type": "PipeWire:Interface:Node", "info": {"props":
            {"media.class": "Audio/Sink", "node.name": "alsa_output.usb-headset"}}}]"#;

        let outputs = Outputs::parse(json).unwrap();

        assert_eq!(descriptions(&outputs), [(56, "alsa_output.usb-headset")]);
    }

    #[test]
    fn text_that_is_not_json_is_an_error() {
        assert!(matches!(
            Outputs::parse("pw-dump: can't connect"),
            Err(DumpError::NotJson(_))
        ));
    }

    #[test]
    fn json_that_is_not_a_list_is_an_error() {
        assert_eq!(Outputs::parse("{}"), Err(DumpError::NotADump));
    }

    #[test]
    fn set_default_is_an_argv_list_with_the_id() {
        assert_eq!(set_default_command(74), ["wpctl", "set-default", "74"]);
    }

    /// A backend that answers reads with `dump`, and keeps each read's and each choice's
    /// callback until the test finishes it, when asked to.
    #[derive(Default)]
    struct Fake {
        dump: RefCell<Option<Dump>>,
        /// Reads held back rather than answered at once.
        hold_reads: Cell<bool>,
        reads: RefCell<Vec<ReadDone>>,
        read_count: Cell<usize>,
        choices: RefCell<Vec<(u32, Done)>>,
    }

    struct FakeBackend(Rc<Fake>);

    impl AudioBackend for FakeBackend {
        fn read(&self, done: ReadDone) {
            self.0.read_count.set(self.0.read_count.get() + 1);
            if self.0.hold_reads.get() {
                self.0.reads.borrow_mut().push(done);
                return;
            }
            let dump = self.0.dump.borrow().clone().expect("a dump to answer with");
            done(dump);
        }

        fn set_default(&self, id: u32, done: Done) {
            self.0.choices.borrow_mut().push((id, done));
        }
    }

    impl Fake {
        fn finish_read(&self, dump: Dump) {
            let done = self.reads.borrow_mut().remove(0);
            done(dump);
        }

        fn finish_choice(&self, result: Result<(), String>) {
            let (_, done) = self.choices.borrow_mut().remove(0);
            done(result);
        }

        fn chosen(&self) -> Vec<u32> {
            self.choices.borrow().iter().map(|(id, _)| *id).collect()
        }
    }

    fn text(name: &str) -> Dump {
        Dump::Text(fixture(name))
    }

    struct Section {
        output: SoundOutput,
        fake: Rc<Fake>,
        failures: Rc<RefCell<Vec<Failure>>>,
        changes: Rc<Cell<usize>>,
    }

    fn section(dump: Dump) -> Section {
        let fake = Rc::new(Fake {
            dump: RefCell::new(Some(dump)),
            ..Fake::default()
        });
        let output = SoundOutput::new(Box::new(FakeBackend(fake.clone())));
        let failures = Rc::new(RefCell::new(Vec::new()));
        output.connect_failed({
            let failures = failures.clone();
            move |failure| failures.borrow_mut().push(failure.clone())
        });
        let changes = Rc::new(Cell::new(0));
        output.connect_changed({
            let changes = changes.clone();
            move |_| changes.set(changes.get() + 1)
        });
        output.refresh();
        Section {
            output,
            fake,
            failures,
            changes,
        }
    }

    #[test]
    fn the_section_is_hidden_until_it_is_read() {
        let fake = Rc::new(Fake::default());
        let output = SoundOutput::new(Box::new(FakeBackend(fake)));

        assert!(!output.state().available);
    }

    #[test]
    fn where_the_tools_cannot_run_the_section_is_hidden_and_nothing_fails() {
        let section = section(Dump::Unavailable);

        assert!(!section.output.state().available);
        assert!(section.failures.borrow().is_empty());
        assert_eq!(section.changes.get(), 0);
    }

    #[test]
    fn outputs_read_show_the_section_with_the_one_in_use() {
        let section = section(text("bluetooth-headset"));

        let state = section.output.state();
        assert!(state.available);
        assert_eq!(state.outputs.in_use().unwrap().description, "WH-1000XM4");
        assert_eq!(section.changes.get(), 1);
        assert!(section.failures.borrow().is_empty());
    }

    #[test]
    fn a_dump_without_outputs_hides_the_section() {
        let section = section(Dump::Text("[]".to_owned()));

        assert!(!section.output.state().available);
        assert!(section.failures.borrow().is_empty());
    }

    #[test]
    fn a_failed_dump_hides_the_section_and_is_reported() {
        let section = section(Dump::Failed("can't connect".to_owned()));

        assert!(!section.output.state().available);
        assert_eq!(
            *section.failures.borrow(),
            [Failure::Read("can't connect".to_owned())]
        );
    }

    #[test]
    fn an_unreadable_dump_hides_the_section_and_is_reported() {
        let section = section(Dump::Text("not json".to_owned()));

        assert!(!section.output.state().available);
        assert!(matches!(
            section.failures.borrow().as_slice(),
            [Failure::Read(_)]
        ));
    }

    #[test]
    fn reading_the_same_outputs_again_changes_nothing() {
        let section = section(text("one-output"));

        section.output.refresh();

        assert_eq!(section.fake.read_count.get(), 2);
        assert_eq!(section.changes.get(), 1);
    }

    #[test]
    fn a_new_default_read_again_is_shown() {
        let section = section(text("one-output"));
        section.fake.dump.replace(Some(text("bluetooth-headset")));

        section.output.refresh();

        assert_eq!(section.output.state().outputs.default, Some(74));
        assert_eq!(section.changes.get(), 2);
    }

    #[test]
    fn outputs_gone_hide_the_section_again() {
        let section = section(text("one-output"));
        section.fake.dump.replace(Some(Dump::Unavailable));

        section.output.refresh();

        assert!(!section.output.state().available);
    }

    #[test]
    fn choosing_an_output_makes_it_the_default_at_once() {
        let section = section(text("several-outputs"));

        section.output.choose(63);

        assert_eq!(section.fake.chosen(), [63]);
        assert_eq!(section.output.state().outputs.default, Some(63));
        assert_eq!(section.changes.get(), 2);
        section.fake.finish_choice(Ok(()));
        assert_eq!(section.output.state().outputs.default, Some(63));
        assert!(section.failures.borrow().is_empty());
    }

    #[test]
    fn choosing_the_output_in_use_does_nothing() {
        let section = section(text("several-outputs"));

        section.output.choose(61);

        assert!(section.fake.chosen().is_empty());
        assert_eq!(section.changes.get(), 1);
    }

    #[test]
    fn choosing_an_id_that_is_no_output_does_nothing() {
        let section = section(text("several-outputs"));

        section.output.choose(57);

        assert!(section.fake.chosen().is_empty());
        assert_eq!(section.output.state().outputs.default, Some(61));
    }

    #[test]
    fn nothing_is_chosen_while_the_section_is_hidden() {
        let section = section(Dump::Unavailable);

        section.output.choose(56);

        assert!(section.fake.chosen().is_empty());
    }

    #[test]
    fn a_choice_that_fails_is_reported_and_the_outputs_are_read_again() {
        let section = section(text("several-outputs"));
        section.output.choose(63);

        section
            .fake
            .finish_choice(Err("Object '63' not found".to_owned()));

        assert_eq!(
            *section.failures.borrow(),
            [Failure::Choose("Object '63' not found".to_owned())]
        );
        assert_eq!(section.fake.read_count.get(), 2);
        assert_eq!(section.output.state().outputs.default, Some(61));
    }

    #[test]
    fn a_read_that_started_before_a_choice_does_not_undo_it() {
        let section = section(text("several-outputs"));
        section.fake.hold_reads.set(true);
        section.output.refresh();

        section.output.choose(63);
        section.fake.finish_read(text("several-outputs"));

        assert_eq!(section.output.state().outputs.default, Some(63));
    }

    #[test]
    fn a_read_that_started_after_a_choice_is_shown() {
        let section = section(text("several-outputs"));
        section.output.choose(63);
        section.fake.hold_reads.set(true);
        section.output.refresh();

        section.fake.finish_read(text("several-outputs"));

        assert_eq!(section.output.state().outputs.default, Some(61));
    }
}
