use cove_ui::ApplicationLaunch;
use cove_ui::compose::primitives::text::Text;

fn application() -> ApplicationLaunch {
    ApplicationLaunch::new(Text::new("MugenZip"))
}

cove_ui::application!(launch = application);
