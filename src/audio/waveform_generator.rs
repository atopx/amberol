// SPDX-FileCopyrightText: 2022  Emmanuele Bassi
// SPDX-License-Identifier: GPL-3.0-or-later

use std::cell::RefCell;

use glib::{clone, Receiver};
use gst::prelude::*;
use gtk::{glib, subclass::prelude::*};

#[derive(Debug)]
pub enum PeaksAction {
    Peak(f64, f64),
    Eos,
}

mod imp {
    use glib::{ParamFlags, ParamSpec, ParamSpecBoolean, Value};
    use once_cell::sync::Lazy;

    use super::*;

    #[derive(Debug)]
    pub struct WaveformGenerator {
        pub uri: RefCell<Option<String>>,
        pub peaks: RefCell<Option<Vec<(f64, f64)>>>,
        pub pipeline: RefCell<Option<gst::Element>>,
        pub receiver: RefCell<Option<Receiver<PeaksAction>>>,
    }

    #[glib::object_subclass]
    impl ObjectSubclass for WaveformGenerator {
        const NAME: &'static str = "WaveformGenerator";
        type Type = super::WaveformGenerator;

        fn new() -> Self {
            Self {
                peaks: RefCell::new(None),
                pipeline: RefCell::new(None),
                receiver: RefCell::new(None),
                uri: RefCell::new(None),
            }
        }
    }

    impl ObjectImpl for WaveformGenerator {
        fn properties() -> &'static [ParamSpec] {
            static PROPERTIES: Lazy<Vec<ParamSpec>> = Lazy::new(|| {
                vec![ParamSpecBoolean::new(
                    "has-peaks",
                    "",
                    "",
                    false,
                    ParamFlags::READABLE,
                )]
            });

            PROPERTIES.as_ref()
        }

        fn property(&self, obj: &Self::Type, _id: usize, pspec: &ParamSpec) -> Value {
            match pspec.name() {
                "has-peaks" => obj.peaks().is_some().to_value(),
                _ => unimplemented!(),
            }
        }
    }
}

glib::wrapper! {
    pub struct WaveformGenerator(ObjectSubclass<imp::WaveformGenerator>);
}

impl Default for WaveformGenerator {
    fn default() -> Self {
        glib::Object::new(&[]).expect("Failed to create WaveformGenerator")
    }
}

impl WaveformGenerator {
    pub fn new() -> Self {
        WaveformGenerator::default()
    }

    pub fn set_uri(&self, uri: Option<String>) {
        self.imp().uri.replace(uri);
    }

    pub fn peaks(&self) -> Option<Vec<(f64, f64)>> {
        (*self.imp().peaks.borrow()).as_ref().cloned()
    }

    pub fn generate_peaks(&self) {
        // Reset the peaks vector
        let peaks: Vec<(f64, f64)> = Vec::new();
        self.imp().peaks.replace(Some(peaks));

        let pipeline_str = "uridecodebin name=uridecodebin ! audioconvert ! audio/x-raw,channels=2 ! level name=level interval=250000000 ! fakesink name=faked";
        let pipeline = match gst::parse_launch(&pipeline_str) {
            Ok(pipeline) => pipeline,
            Err(err) => {
                warn!("Unable to generate peaks: {}", err);
                return;
            }
        };

        let uridecodebin = pipeline
            .downcast_ref::<gst::Bin>()
            .unwrap()
            .by_name("uridecodebin")
            .unwrap();
        uridecodebin.set_property("uri", self.imp().uri.borrow().as_deref());

        let fakesink = pipeline
            .downcast_ref::<gst::Bin>()
            .unwrap()
            .by_name("faked")
            .unwrap();
        fakesink.set_property("qos", false);
        fakesink.set_property("sync", false);

        let bus = pipeline
            .bus()
            .expect("Pipeline without bus. Shouldn't happen!");

        let (sender, receiver) = glib::MainContext::channel(glib::PRIORITY_DEFAULT);
        self.imp().receiver.replace(Some(receiver));

        self.imp().receiver.borrow_mut().take().unwrap().attach(
            None,
            clone!(@strong self as this => move |action| {
                match action {
                    PeaksAction::Peak(p1, p2) => {
                        if let Some(ref mut peaks) = *this.imp().peaks.borrow_mut() {
                            peaks.push((p1, p2));
                        }
                    }
                    PeaksAction::Eos => {
                        // We're done
                        this.notify("has-peaks");
                        return glib::Continue(false);
                    }
                }

                glib::Continue(true)
            }),
        );

        // Use a weak reference in the closure
        let pipeline_weak = pipeline.downgrade();

        debug!("Adding bus watch");
        bus.add_watch(clone!(@strong sender => move |_, msg| {
            use gst::MessageView;

            // If the pipeline was dropped, we drop the watch as well
            let pipeline = match pipeline_weak.upgrade() {
                Some(pipeline) => pipeline,
                None => return glib::Continue(false),
            };

            match msg.view() {
                MessageView::Eos(..) => {
                    debug!("End of waveform stream");
                    pipeline.set_state(gst::State::Null).expect("Unable to set 'null' state");
                    send!(sender, PeaksAction::Eos);
                    return glib::Continue(false);
                }
                MessageView::Error(err) => {
                    warn!("Pipeline error: {:?}", err);
                    pipeline.set_state(gst::State::Ready).expect("Unable to set 'null' state");
                    send!(sender, PeaksAction::Eos);
                    return glib::Continue(false);
                }
                MessageView::Element(element) => {
                    if let Some(s) = element.structure() {
                        if s.has_name("level") {
                            let peaks_array = s.get::<&glib::ValueArray>("peak").unwrap();
                            let v1 = peaks_array[0].get::<f64>().unwrap();
                            let v2 = peaks_array[1].get::<f64>().unwrap();
                            // Normalize peaks between 0 and 1
                            let peak1 = f64::powf(10.0, v1 / 20.0);
                            let peak2 = f64::powf(10.0, v2 / 20.0);
                            send!(sender, PeaksAction::Peak(peak1, peak2));
                        }
                    }
                }
                _ => (),
            };

            glib::Continue(true)
        }))
        .expect("failed to add bus watch");

        pipeline
            .set_state(gst::State::Playing)
            .expect("Failed to play pipeline");

        // Keep a reference on the pipeline so we can run it until completion
        self.imp().pipeline.replace(Some(pipeline));
    }
}