use std::{
    cell::{OnceCell, RefCell},
    collections::VecDeque,
    time::Instant,
};

use objc2::{
    AnyThread, MainThreadMarker, MainThreadOnly, msg_send,
    rc::Retained,
    runtime::{AnyObject, ProtocolObject, Sel},
    sel,
};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationIcon, NSAboutPanelOptionApplicationName,
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionCredits, NSAboutPanelOptionKey,
    NSAboutPanelOptionVersion, NSApplication, NSApplicationActivationPolicy, NSBitmapImageFileType,
    NSBitmapImageRep, NSColor, NSControlStateValueOff, NSControlStateValueOn, NSEventModifierFlags,
    NSFont, NSFontAttributeName, NSFontWeightRegular, NSForegroundColorAttributeName, NSImage,
    NSImageSymbolConfiguration, NSLinkAttributeName, NSMenu, NSMenuItem, NSMutableParagraphStyle,
    NSParagraphStyleAttributeName, NSTextAlignment,
};
use objc2_foundation::{
    NSCopying, NSData, NSDictionary, NSMutableAttributedString, NSMutableDictionary, NSObject,
    NSRange, NSString, ns_string,
};
use winit::event_loop::EventLoopProxy;

use crate::{commands::Command, metadata};

/// Cache the OS's symbol artwork per egui context. Keep the mask white so the
/// same Retina-resolution texture can follow selection, appearance, and scale.
pub(crate) fn paint_symbol(
    ui: &egui::Ui,
    name: &str,
    origin: egui::Pos2,
    size: f32,
    color: egui::Color32,
) -> bool {
    let id = egui::Id::new(("sf_symbol", name));
    let cached = ui
        .ctx()
        .data(|data| data.get_temp::<Option<egui::TextureHandle>>(id));
    let texture = cached.unwrap_or_else(|| {
        let texture = symbol_image(name).map(|image| {
            ui.ctx().load_texture(
                format!("sf_symbol/{name}"),
                image,
                egui::TextureOptions::LINEAR,
            )
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(id, texture.clone()));
        texture
    });
    let Some(texture) = texture else { return false };
    let image_size = texture.size_vec2();
    let fitted = image_size * (size / image_size.max_elem());
    ui.painter().image(
        texture.id(),
        egui::Rect::from_center_size(origin + egui::Vec2::splat(size * 0.5), fitted),
        egui::Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
        color,
    );
    true
}

fn symbol_image(name: &str) -> Option<egui::ColorImage> {
    objc2::rc::autoreleasepool(|_| {
        let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
            &NSString::from_str(name),
            None,
        )?;
        let config = NSImageSymbolConfiguration::configurationWithPointSize_weight(32.0, unsafe {
            NSFontWeightRegular
        })
        .configurationByApplyingConfiguration(
            &NSImageSymbolConfiguration::configurationWithHierarchicalColor(&NSColor::whiteColor()),
        );
        let image = image.imageWithSymbolConfiguration(&config)?;
        let tiff = image.TIFFRepresentation()?;
        let bitmap = NSBitmapImageRep::imageRepWithData(&tiff)?;
        // An empty properties dictionary requests the standard lossless PNG representation.
        let data = unsafe {
            bitmap.representationUsingType_properties(
                NSBitmapImageFileType::PNG,
                &NSDictionary::new(),
            )
        }?;
        let pixels = image::load_from_memory(&data.to_vec()).ok()?.into_rgba8();
        Some(egui::ColorImage::from_rgba_unmultiplied(
            [pixels.width() as usize, pixels.height() as usize],
            &pixels,
        ))
    })
}

const ABOUT: isize = 1;
const SETTINGS: isize = 2;
const OPEN: isize = 4;
const SAVE: isize = 5;
const SAVE_AS: isize = 6;
const CLOSE_WORKSPACE: isize = 7;
const QUIT: isize = 8;
const SIDEBAR: isize = 9;
const INSPECTOR: isize = 10;
const ACTIVITY: isize = 11;
const UNDO: isize = 12;
const REDO: isize = 13;
const CUT: isize = 14;
const COPY: isize = 15;
const PASTE: isize = 16;
const SELECT_ALL: isize = 17;
const ANDROID: isize = 18;
const APPLE: isize = 19;
const WIZARD: isize = 20;
const SALES: isize = 21;

#[derive(Clone, Copy, Default)]
pub(crate) struct MenuState {
    pub has_workspace: bool,
    pub has_selected_app: bool,
    pub has_pending_action: bool,
    pub show_sidebar: bool,
    pub show_inspector: bool,
    pub show_activity: bool,
}

pub(crate) enum MenuAction {
    Command(Command),
    Edit(EditAction),
}
pub(crate) enum EditAction {
    Undo,
    Redo,
    Cut,
    Copy,
    Paste,
    SelectAll,
}

thread_local! {
    static TARGET: OnceCell<Retained<MenuTarget>> = const { OnceCell::new() };
    static ACTIONS: RefCell<VecDeque<MenuAction>> = const { RefCell::new(VecDeque::new()) };
    static PROXY: RefCell<Option<EventLoopProxy<Instant>>> = const { RefCell::new(None) };
    static MENU_STATE: std::cell::Cell<MenuState> = const { std::cell::Cell::new(MenuState { has_workspace: false, has_selected_app: false, has_pending_action: false, show_sidebar: false, show_inspector: false, show_activity: false }) };
    static EDITABLE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(validateMenuItem:))]
        fn validate_menu_item(&self, item: &NSMenuItem) -> bool {
            let state = MENU_STATE.with(std::cell::Cell::get);
            match item.tag() {
                SAVE | SAVE_AS => state.has_workspace && !state.has_pending_action,
                CLOSE_WORKSPACE => state.has_selected_app && !state.has_pending_action,
                OPEN | QUIT => !state.has_pending_action,
                UNDO | REDO | CUT | COPY | PASTE | SELECT_ALL => EDITABLE.with(std::cell::Cell::get),
                ABOUT | SETTINGS | SIDEBAR | INSPECTOR | ACTIVITY => true,
                _ => true,
            }
        }

        #[unsafe(method(performStudioMenuAction:))]
        fn perform_studio_menu_action(&self, sender: &NSMenuItem) {
            let action = match sender.tag() {
                ABOUT => Some(MenuAction::Command(Command::About)),
                SETTINGS => Some(MenuAction::Command(Command::Settings)),
                ANDROID => Some(MenuAction::Command(Command::Android)),
                APPLE => Some(MenuAction::Command(Command::Apple)),
                WIZARD => Some(MenuAction::Command(Command::Wizard)),
                SALES => Some(MenuAction::Command(Command::Sales)),
                OPEN => Some(MenuAction::Command(Command::OpenWorkspace)),
                SAVE => Some(MenuAction::Command(Command::Save)),
                SAVE_AS => Some(MenuAction::Command(Command::SaveAs)),
                CLOSE_WORKSPACE => Some(MenuAction::Command(Command::CloseWorkspace)),
                QUIT => Some(MenuAction::Command(Command::Quit)),
                SIDEBAR => Some(MenuAction::Command(Command::ToggleSidebar)),
                INSPECTOR => Some(MenuAction::Command(Command::ToggleInspector)),
                ACTIVITY => Some(MenuAction::Command(Command::ToggleActivity)),
                UNDO => Some(MenuAction::Edit(EditAction::Undo)),
                REDO => Some(MenuAction::Edit(EditAction::Redo)),
                CUT => Some(MenuAction::Edit(EditAction::Cut)),
                COPY => Some(MenuAction::Edit(EditAction::Copy)),
                PASTE => Some(MenuAction::Edit(EditAction::Paste)),
                SELECT_ALL => Some(MenuAction::Edit(EditAction::SelectAll)),
                _ => None,
            };
            if let Some(action) = action {
                ACTIONS.with(|actions| actions.borrow_mut().push_back(action));
                PROXY.with(|proxy| if let Some(proxy) = proxy.borrow().as_ref() { let _ = proxy.send_event(Instant::now()); });
            }
        }
    }
);

impl MenuTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's initializer is valid for this ivar-free subclass.
        unsafe { msg_send![super(this), init] }
    }
}

pub(crate) fn take_menu_action() -> Option<MenuAction> {
    ACTIONS.with(|actions| actions.borrow_mut().pop_front())
}

pub(crate) fn install_native_menu(proxy: EventLoopProxy<Instant>) {
    PROXY.with(|slot| *slot.borrow_mut() = Some(proxy));
    let mtm = MainThreadMarker::new().expect("AppKit menu setup runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Regular);
    let Some(main_menu) = app.mainMenu() else {
        return;
    };
    let Some(app_item) = main_menu.itemAtIndex(0) else {
        return;
    };
    let Some(app_menu) = app_item.submenu() else {
        return;
    };
    let target = TARGET.with(|target| target.get_or_init(|| MenuTarget::new(mtm)).clone());
    let target_obj: &AnyObject = &target;

    app_item.setTitle(&NSString::from_str(metadata::APP_NAME));
    app_menu.setTitle(&NSString::from_str(metadata::APP_NAME));
    if let Some(item) = app_menu.itemAtIndex(0) {
        item.setTitle(&NSString::from_str(&format!(
            "About {}",
            metadata::APP_NAME
        )));
        // SAFETY: selector is registered on the retained MenuTarget below.
        unsafe {
            item.setTarget(Some(target_obj));
            item.setAction(Some(sel!(performStudioMenuAction:)));
        }
        item.setTag(ABOUT);
    }
    // Keep winit's Services, Hide, Hide Others, Show All, and Quit items. Give
    // the standard process-derived labels the product name used by the window.
    for index in 0..app_menu.numberOfItems() {
        if let Some(item) = app_menu.itemAtIndex(index) {
            if item.action() == Some(sel!(hide:)) {
                item.setTitle(&NSString::from_str(&format!("Hide {}", metadata::APP_NAME)));
            }
            if item.action() == Some(sel!(terminate:)) {
                item.setTitle(&NSString::from_str(&format!("Quit {}", metadata::APP_NAME)));
                // Replace the stock action so Quit reaches the workspace guard.
                // SAFETY: selector is registered on the retained MenuTarget.
                unsafe {
                    item.setTarget(Some(target_obj));
                    item.setAction(Some(sel!(performStudioMenuAction:)));
                }
                item.setTag(QUIT);
            }
        }
    }
    let settings = menu_item(
        mtm,
        target_obj,
        "Settings…",
        ",",
        SETTINGS,
        NSEventModifierFlags::Command,
    );
    app_menu.insertItem_atIndex(&settings, 2);
    app_menu.insertItem_atIndex(&NSMenuItem::separatorItem(mtm), 3);

    let file = NSMenu::new(mtm);
    file.setTitle(ns_string!("File"));
    for (title, tag) in [
        ("Wizard", WIZARD),
        ("Android", ANDROID),
        ("Apple", APPLE),
        ("Sales", SALES),
    ] {
        add(
            &file,
            menu_item(
                mtm,
                target_obj,
                title,
                "",
                tag,
                NSEventModifierFlags::empty(),
            ),
        );
    }
    add(&file, NSMenuItem::separatorItem(mtm));
    add(
        &file,
        menu_item(
            mtm,
            target_obj,
            "Open App…",
            "o",
            OPEN,
            NSEventModifierFlags::Command,
        ),
    );
    add(&file, NSMenuItem::separatorItem(mtm));
    add(
        &file,
        menu_item(
            mtm,
            target_obj,
            "Save",
            "s",
            SAVE,
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &file,
        menu_item(
            mtm,
            target_obj,
            "Save As…",
            "s",
            SAVE_AS,
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        ),
    );
    add(&file, NSMenuItem::separatorItem(mtm));
    add(
        &file,
        menu_item(
            mtm,
            target_obj,
            "Close App",
            "w",
            CLOSE_WORKSPACE,
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        ),
    );
    add(
        &file,
        standard_item(
            mtm,
            "Close Window",
            "w",
            sel!(performClose:),
            NSEventModifierFlags::Command,
        ),
    );
    if let Some(close_window) = file.itemAtIndex(file.numberOfItems() - 1) {
        close_window.setEnabled(true);
    }
    add_top_menu(&main_menu, mtm, "File", &file);

    let edit = NSMenu::new(mtm);
    edit.setTitle(ns_string!("Edit"));
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Undo",
            "z",
            UNDO,
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Redo",
            "z",
            REDO,
            NSEventModifierFlags::Command | NSEventModifierFlags::Shift,
        ),
    );
    add(&edit, NSMenuItem::separatorItem(mtm));
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Cut",
            "x",
            CUT,
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Copy",
            "c",
            COPY,
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Paste",
            "v",
            PASTE,
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &edit,
        menu_item(
            mtm,
            target_obj,
            "Select All",
            "a",
            SELECT_ALL,
            NSEventModifierFlags::Command,
        ),
    );
    add_top_menu(&main_menu, mtm, "Edit", &edit);

    let view = NSMenu::new(mtm);
    view.setTitle(ns_string!("View"));
    add(
        &view,
        menu_item(
            mtm,
            target_obj,
            "Recent Workspaces",
            "",
            SIDEBAR,
            NSEventModifierFlags::empty(),
        ),
    );
    add(
        &view,
        menu_item(
            mtm,
            target_obj,
            "Workspace Details",
            "",
            INSPECTOR,
            NSEventModifierFlags::empty(),
        ),
    );
    add(
        &view,
        menu_item(
            mtm,
            target_obj,
            "Activity",
            "",
            ACTIVITY,
            NSEventModifierFlags::empty(),
        ),
    );
    add_top_menu(&main_menu, mtm, "View", &view);

    let window = NSMenu::new(mtm);
    window.setTitle(ns_string!("Window"));
    add(
        &window,
        standard_item(
            mtm,
            "Minimize",
            "m",
            sel!(performMiniaturize:),
            NSEventModifierFlags::Command,
        ),
    );
    add(
        &window,
        standard_item(
            mtm,
            "Zoom",
            "",
            sel!(performZoom:),
            NSEventModifierFlags::empty(),
        ),
    );
    add(&window, NSMenuItem::separatorItem(mtm));
    add(
        &window,
        standard_item(
            mtm,
            "Bring All to Front",
            "",
            sel!(arrangeInFront:),
            NSEventModifierFlags::empty(),
        ),
    );
    add_top_menu(&main_menu, mtm, "Window", &window);
    app.setWindowsMenu(Some(&window));
}

pub(crate) fn set_application_icon() {
    let mtm = MainThreadMarker::new().expect("AppKit icon setup runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let bytes = NSData::with_bytes(metadata::LOGO_BYTES);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(), &bytes) {
        // SAFETY: NSApplication retains the live NSImage for its dock icon.
        unsafe {
            app.setApplicationIconImage(Some(&image));
        }
    }
}

pub(crate) fn show_about_panel() {
    let mtm = MainThreadMarker::new().expect("AppKit About panel runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let bytes = NSData::with_bytes(metadata::LOGO_BYTES);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &bytes) else {
        return;
    };
    let name = NSString::from_str(metadata::APP_NAME);
    let version = NSString::from_str("");
    let build = NSString::from_str(metadata::GIT_SHA);
    let credits = about_panel_credits();
    let options = NSMutableDictionary::<NSAboutPanelOptionKey, AnyObject>::new();
    // SAFETY: AppKit's exported option keys are immutable NSString constants.
    unsafe {
        set_about_option(&options, NSAboutPanelOptionApplicationName, &name);
        set_about_option(&options, NSAboutPanelOptionApplicationIcon, &image);
        set_about_option(&options, NSAboutPanelOptionApplicationVersion, &version);
        set_about_option(&options, NSAboutPanelOptionVersion, &build);
        set_about_option(&options, NSAboutPanelOptionCredits, &credits);
    }
    // SAFETY: each key conforms to NSCopying and each value has the documented
    // NSString/NSImage type for the About panel options dictionary.
    unsafe {
        app.orderFrontStandardAboutPanelWithOptions(&options);
    }
}

fn about_panel_credits() -> Retained<NSMutableAttributedString> {
    let text = NSString::from_str("Website\nGitHub");
    let credits =
        NSMutableAttributedString::initWithString(NSMutableAttributedString::alloc(), &text);
    let full_range = NSRange::from(0..text.length());
    let paragraph = NSMutableParagraphStyle::new();
    paragraph.setAlignment(NSTextAlignment::Center);
    paragraph.setLineSpacing(8.0);
    let color = NSColor::linkColor();
    let font = NSFont::systemFontOfSize(13.0);
    // SAFETY: the attributes use AppKit's documented object types and ranges
    // match the ASCII credit text in UTF-16 code units.
    unsafe {
        credits.addAttribute_value_range(NSParagraphStyleAttributeName, &paragraph, full_range);
        credits.addAttribute_value_range(NSForegroundColorAttributeName, &color, full_range);
        credits.addAttribute_value_range(NSFontAttributeName, &font, full_range);

        let website = NSString::from_str(metadata::WEBSITE_URL);
        credits.addAttribute_value_range(NSLinkAttributeName, &website, NSRange::from(0..7));
        let github = NSString::from_str(metadata::GITHUB_URL);
        credits.addAttribute_value_range(NSLinkAttributeName, &github, NSRange::from(8..14));
    }
    credits
}

fn set_about_option(
    options: &NSMutableDictionary<NSAboutPanelOptionKey, AnyObject>,
    key: &NSAboutPanelOptionKey,
    value: &AnyObject,
) {
    let key: &ProtocolObject<dyn NSCopying> = ProtocolObject::from_ref(key);
    // SAFETY: call sites pass NSString or NSImage, both Objective-C objects.
    unsafe {
        options.setObject_forKey(value, key);
    }
}

fn menu_item(
    mtm: MainThreadMarker,
    target: &AnyObject,
    title: &str,
    key: &str,
    tag: isize,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    let item = standard_item(mtm, title, key, sel!(performStudioMenuAction:), modifiers);
    item.setTag(tag);
    // SAFETY: selector is registered on the retained MenuTarget.
    unsafe {
        item.setTarget(Some(target));
    }
    item
}

fn standard_item(
    mtm: MainThreadMarker,
    title: &str,
    key: &str,
    action: Sel,
    modifiers: NSEventModifierFlags,
) -> Retained<NSMenuItem> {
    // SAFETY: every selector here has NSMenuItem's one-argument action signature.
    let item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            Some(action),
            &NSString::from_str(key),
        )
    };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

fn add(menu: &NSMenu, item: Retained<NSMenuItem>) {
    menu.addItem(&item);
}

fn add_top_menu(main: &NSMenu, mtm: MainThreadMarker, title: &str, submenu: &NSMenu) {
    let item = NSMenuItem::new(mtm);
    item.setTitle(&NSString::from_str(title));
    item.setSubmenu(Some(submenu));
    main.addItem(&item);
}

fn checked(menu: &NSMenu, tag: isize, enabled: bool, value: Option<bool>) {
    if let Some(item) = menu.itemWithTag(tag) {
        item.setEnabled(enabled);
        if let Some(value) = value {
            item.setState(if value {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
        }
    }
}

pub(crate) fn update_menu_state(state: MenuState, wants_keyboard_input: bool) {
    MENU_STATE.with(|snapshot| snapshot.set(state));
    EDITABLE.with(|editable| editable.set(wants_keyboard_input));
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let Some(main) = app.mainMenu() else { return };
    for index in 0..main.numberOfItems() {
        let Some(menu) = main.itemAtIndex(index).and_then(|item| item.submenu()) else {
            continue;
        };
        checked(
            &menu,
            SAVE,
            state.has_workspace && !state.has_pending_action,
            None,
        );
        checked(
            &menu,
            SAVE_AS,
            state.has_workspace && !state.has_pending_action,
            None,
        );
        checked(
            &menu,
            CLOSE_WORKSPACE,
            state.has_selected_app && !state.has_pending_action,
            None,
        );
        checked(&menu, OPEN, !state.has_pending_action, None);
        checked(&menu, QUIT, !state.has_pending_action, None);
        checked(&menu, SIDEBAR, true, Some(state.show_sidebar));
        checked(&menu, INSPECTOR, true, Some(state.show_inspector));
        checked(&menu, ACTIVITY, true, Some(state.show_activity));
        for tag in [UNDO, REDO, CUT, COPY, PASTE, SELECT_ALL] {
            checked(&menu, tag, wants_keyboard_input, None);
        }
    }
}
