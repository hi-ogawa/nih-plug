//! And [`Editor`] implementation for iced.

use baseview::{WindowOpenOptions, WindowScalePolicy};
use crossbeam::atomic::AtomicCell;
use crossbeam::channel;
pub use iced_baseview::*;
use nih_plug::prelude::{Editor, GuiContext, ParentWindowHandle};
use std::sync::atomic::Ordering;
use std::sync::Arc;

use crate::{wrapper, IcedEditor, IcedState, ParameterUpdate};

/// An [`Editor`] implementation that renders an iced [`Application`].
pub(crate) struct IcedEditorWrapper<E: IcedEditor> {
    pub(crate) iced_state: Arc<IcedState>,
    pub(crate) initialization_flags: E::InitializationFlags,

    /// The scaling factor reported by the host, if any. On macOS this will never be set and we
    /// should use the system scaling factor instead.
    pub(crate) scaling_factor: AtomicCell<Option<f32>>,

    /// A subscription for sending messages about parameter updates to the `IcedEditor`.
    pub(crate) parameter_updates_sender: channel::Sender<ParameterUpdate>,
    pub(crate) parameter_updates_receiver: Arc<channel::Receiver<ParameterUpdate>>,
}

impl<E: IcedEditor> Editor for IcedEditorWrapper<E> {
    fn spawn(
        &self,
        parent: ParentWindowHandle,
        context: Arc<dyn GuiContext>,
    ) -> Box<dyn std::any::Any + Send> {
        let (unscaled_width, unscaled_height) = self.iced_state.size();
        let scaling_factor = self.scaling_factor.load();

        // TODO: iced_baseview does not have gracefuly error handling for context creation failures.
        //       This will panic if the context could not be created.
        let window = IcedWindow::<wrapper::IcedEditorWrapperApplication<E>>::open_parented(
            &parent,
            Settings {
                window: WindowOpenOptions {
                    title: String::from("iced window"),
                    // Baseview should be doing the DPI scaling for us
                    size: baseview::Size::new(unscaled_width as f64, unscaled_height as f64),
                    // NOTE: For some reason passing 1.0 here causes the UI to be scaled on macOS but
                    //       not the mouse events.
                    scale: scaling_factor
                        .map(|factor| WindowScalePolicy::ScaleFactor(factor as f64))
                        .unwrap_or(WindowScalePolicy::SystemScaleFactor),

                    #[cfg(feature = "opengl")]
                    gl_config: Some(baseview::gl::GlConfig {
                        // FIXME: glow_glyph forgot to add an `#extension`, so this won't work under
                        //        OpenGL 3.2 at the moment. With that change applied this should work on
                        //        OpenGL 3.2/macOS.
                        version: (3, 3),
                        red_bits: 8,
                        blue_bits: 8,
                        green_bits: 8,
                        alpha_bits: 8,
                        depth_bits: 24,
                        stencil_bits: 8,
                        samples: None,
                        srgb: true,
                        double_buffer: true,
                        vsync: true,
                        ..Default::default()
                    }),
                    // FIXME: Rust analyzer always thinks baseview/opengl is enabled even if we
                    //        don't explicitly enable it, so you'd get a compile error if this line
                    //        is missing
                    #[cfg(not(feature = "opengl"))]
                    gl_config: None,
                },
                iced_baseview: IcedBaseviewSettings {
                    ignore_non_modifier_keys: false,
                    always_redraw: true,
                },
                // We use this wrapper to be able to pass the GUI context to the editor
                flags: (
                    context,
                    self.parameter_updates_receiver.clone(),
                    self.initialization_flags.clone(),
                ),
            },
        );

        self.iced_state.open.store(true, Ordering::Release);
        Box::new(IcedEditorHandle {
            iced_state: self.iced_state.clone(),
            window,
        })
    }

    fn size(&self) -> (u32, u32) {
        self.iced_state.size()
    }

    fn set_scale_factor(&self, factor: f32) -> bool {
        self.scaling_factor.store(Some(factor));
        true
    }

    fn param_values_changed(&self) {
        if self.iced_state.is_open() {
            // If there's already a paramter change notification in the channel then we don't need
            // to do anything else. This avoids queueing up redundant GUI redraws.
            let _ = self.parameter_updates_sender.try_send(ParameterUpdate);
        }
    }
}

/// The window handle used for [`IcedEditorWrapper`].
struct IcedEditorHandle<Message: 'static + Send> {
    iced_state: Arc<IcedState>,
    window: iced_baseview::WindowHandle<Message>,
}

/// The window handle enum stored within 'WindowHandle' contains raw pointers. Is there a way around
/// having this requirement?
unsafe impl<Message: Send> Send for IcedEditorHandle<Message> {}

impl<Message: Send> Drop for IcedEditorHandle<Message> {
    fn drop(&mut self) {
        self.iced_state.open.store(false, Ordering::Release);
        self.window.close_window();
    }
}
