//! ECS rendering bundle

use amethyst_assets::{AssetStorage, Handle, Processor};
use amethyst_core::bundle::{ECSBundle, Result};
use shrev::EventChannel;
use specs::{DispatcherBuilder, World};
use winit::Event;

use super::*;

/// UI bundle
///
/// Will register all necessary components and systems needed for UI, along with any resources.
///
/// `UiTextRenderer` is registered with name `"ui_text"`.
pub struct UiBundle {
    deps: &'static [&'static str],
}

impl UiBundle {
    /// Create a new UI bundle, the dependencies given will be the dependencies for the
    /// UiTextRenderer system.
    pub fn new(deps: &'static [&'static str]) -> Self {
        UiBundle { deps }
    }
}

impl<'a, 'b> ECSBundle<'a, 'b> for UiBundle {
    fn build(
        self,
        world: &mut World,
        builder: DispatcherBuilder<'a, 'b>,
    ) -> Result<DispatcherBuilder<'a, 'b>> {
        world.register::<UiImage>();
        world.register::<UiTransform>();
        world.register::<UiText>();
        world.register::<UiResize>();
        world.register::<Handle<FontAsset>>();
        world.add_resource(AssetStorage::<FontAsset>::new());
        let reader = world
            .read_resource::<EventChannel<Event>>()
            .register_reader();
        Ok(
            builder
                .add(UiTextRenderer, "ui_text", self.deps)
                .add(Processor::<FontAsset>::new(), "font_processor", &[])
                .add(ResizeSystem::new(reader), "ui_resize_system", &[]),
        )
    }
}