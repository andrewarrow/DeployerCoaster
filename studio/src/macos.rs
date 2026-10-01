use std::{cell::{OnceCell, RefCell}, collections::VecDeque, time::Instant};

use objc2::{AnyThread, MainThreadMarker, msg_send, rc::Retained, runtime::{AnyObject, ProtocolObject}, sel};
use objc2_app_kit::{
    NSAboutPanelOptionApplicationIcon, NSAboutPanelOptionApplicationName,
    NSAboutPanelOptionApplicationVersion, NSAboutPanelOptionKey, NSAboutPanelOptionVersion,
    NSApplication, NSApplicationActivationPolicy, NSEventModifierFlags, NSImage, NSMenu,
    NSMenuItem, NSControlStateValue, NSControlStateValueOff, NSControlStateValueOn,
};
use objc2_foundation::{NSData, NSDictionary, NSMutableDictionary, NSCopying, NSObject, NSString, ns_string};
use winit::event_loop::EventLoopProxy;

use crate::{commands::Command, metadata};

const ABOUT: isize = 1;
const SETTINGS: isize = 2;
const NEW: isize = 3;
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

pub(crate) struct MenuState {
    pub has_workspace: bool,
    pub has_pending_action: bool,
    pub show_sidebar: bool,
    pub show_inspector: bool,
    pub show_activity: bool,
}

pub(crate) enum MenuAction { Command(Command), Edit(EditAction) }
pub(crate) enum EditAction { Undo, Redo, Cut, Copy, Paste, SelectAll }

thread_local! {
    static TARGET: OnceCell<Retained<MenuTarget>> = const { OnceCell::new() };
    static ACTIONS: RefCell<VecDeque<MenuAction>> = const { RefCell::new(VecDeque::new()) };
    static PROXY: RefCell<Option<EventLoopProxy<Instant>>> = const { RefCell::new(None) };
}

objc2::define_class!(
    #[unsafe(super = NSObject)]
    #[thread_kind = MainThreadOnly]
    #[ivars = ()]
    struct MenuTarget;

    impl MenuTarget {
        #[unsafe(method(performStudioMenuAction:))]
        fn perform_studio_menu_action(&self, sender: &NSMenuItem) {
            let action = match sender.tag() {
                ABOUT => Some(MenuAction::Command(Command::About)),
                SETTINGS => Some(MenuAction::Command(Command::Settings)),
                NEW => Some(MenuAction::Command(Command::NewWorkspace)),
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
    let Some(main_menu) = app.mainMenu() else { return };
    let Some(app_item) = main_menu.itemAtIndex(0) else { return };
    let Some(app_menu) = app_item.submenu() else { return };
    let target = TARGET.with(|target| target.get_or_init(|| MenuTarget::new(mtm)).clone());
    let target_obj: &AnyObject = &target;

    app_item.setTitle(&NSString::from_str(metadata::APP_NAME));
    app_menu.setTitle(&NSString::from_str(metadata::APP_NAME));
    if let Some(item) = app_menu.itemAtIndex(0) {
        item.setTitle(&NSString::from_str(&format!("About {}", metadata::APP_NAME)));
        // SAFETY: selector is registered on the retained MenuTarget below.
        unsafe { item.setTarget(Some(target_obj)); item.setAction(Some(sel!(performStudioMenuAction:))); }
        item.setTag(ABOUT);
    }
    // Keep winit's Services, Hide, Hide Others, Show All, and Quit items. Give
    // the standard process-derived labels the product name used by the window.
    for index in 0..app_menu.numberOfItems() {
        if let Some(item) = app_menu.itemAtIndex(index) {
            let title = item.title().to_string();
            if title == "Hide" { item.setTitle(&NSString::from_str(&format!("Hide {}", metadata::APP_NAME))); }
            if title == "Quit" {
                item.setTitle(&NSString::from_str(&format!("Quit {}", metadata::APP_NAME)));
                // Replace the stock action so Quit reaches the workspace guard.
                // SAFETY: selector is registered on the retained MenuTarget.
                unsafe { item.setTarget(Some(target_obj)); item.setAction(Some(sel!(performStudioMenuAction:))); }
                item.setTag(QUIT);
            }
        }
    }
    let settings = menu_item(mtm, target_obj, "Settings…", ",", SETTINGS, NSEventModifierFlags::Command);
    app_menu.insertItem_atIndex(&settings, 1);

    let file = NSMenu::new(mtm);
    file.setTitle(ns_string!("File"));
    add(&file, menu_item(mtm, target_obj, "New Workspace", "n", NEW, NSEventModifierFlags::Command));
    add(&file, menu_item(mtm, target_obj, "Open…", "o", OPEN, NSEventModifierFlags::Command));
    add(&file, NSMenuItem::separatorItem(mtm));
    add(&file, menu_item(mtm, target_obj, "Save", "s", SAVE, NSEventModifierFlags::Command));
    add(&file, menu_item(mtm, target_obj, "Save As…", "s", SAVE_AS, NSEventModifierFlags::Command | NSEventModifierFlags::Shift));
    add(&file, NSMenuItem::separatorItem(mtm));
    add(&file, menu_item(mtm, target_obj, "Close Workspace", "w", CLOSE_WORKSPACE, NSEventModifierFlags::Command | NSEventModifierFlags::Shift));
    add(&file, standard_item(mtm, "Close Window", "w", sel!(performClose:), NSEventModifierFlags::Command));
    add_top_menu(&main_menu, mtm, "File", &file);

    let edit = NSMenu::new(mtm);
    edit.setTitle(ns_string!("Edit"));
    add(&edit, menu_item(mtm, target_obj, "Undo", "z", UNDO, NSEventModifierFlags::Command));
    add(&edit, menu_item(mtm, target_obj, "Redo", "z", REDO, NSEventModifierFlags::Command | NSEventModifierFlags::Shift));
    add(&edit, NSMenuItem::separatorItem(mtm));
    add(&edit, menu_item(mtm, target_obj, "Cut", "x", CUT, NSEventModifierFlags::Command));
    add(&edit, menu_item(mtm, target_obj, "Copy", "c", COPY, NSEventModifierFlags::Command));
    add(&edit, menu_item(mtm, target_obj, "Paste", "v", PASTE, NSEventModifierFlags::Command));
    add(&edit, menu_item(mtm, target_obj, "Select All", "a", SELECT_ALL, NSEventModifierFlags::Command));
    add_top_menu(&main_menu, mtm, "Edit", &edit);

    let view = NSMenu::new(mtm);
    view.setTitle(ns_string!("View"));
    add(&view, menu_item(mtm, target_obj, "Recent Workspaces", "", SIDEBAR, NSEventModifierFlags::empty()));
    add(&view, menu_item(mtm, target_obj, "Workspace Details", "", INSPECTOR, NSEventModifierFlags::empty()));
    add(&view, menu_item(mtm, target_obj, "Activity", "", ACTIVITY, NSEventModifierFlags::empty()));
    add_top_menu(&main_menu, mtm, "View", &view);

    let window = NSMenu::new(mtm);
    window.setTitle(ns_string!("Window"));
    add(&window, standard_item(mtm, "Minimize", "m", sel!(performMiniaturize:), NSEventModifierFlags::Command));
    add(&window, standard_item(mtm, "Zoom", "", sel!(performZoom:), NSEventModifierFlags::empty()));
    add(&window, NSMenuItem::separatorItem(mtm));
    add(&window, standard_item(mtm, "Bring All to Front", "", sel!(arrangeInFront:), NSEventModifierFlags::empty()));
    add_top_menu(&main_menu, mtm, "Window", &window);
    app.setWindowsMenu(Some(&window));
}

pub(crate) fn set_application_icon() {
    let mtm = MainThreadMarker::new().expect("AppKit icon setup runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let bytes = NSData::with_bytes(metadata::LOGO_BYTES);
    if let Some(image) = NSImage::initWithData(NSImage::alloc(mtm), &bytes) {
        // SAFETY: NSApplication retains the live NSImage for its dock icon.
        unsafe { app.setApplicationIconImage(Some(&image)); }
    }
}

pub(crate) fn show_about_panel() {
    let mtm = MainThreadMarker::new().expect("AppKit About panel runs on the main thread");
    let app = NSApplication::sharedApplication(mtm);
    let bytes = NSData::with_bytes(metadata::LOGO_BYTES);
    let Some(image) = NSImage::initWithData(NSImage::alloc(mtm), &bytes) else { return };
    let name = NSString::from_str(metadata::APP_NAME);
    let version = NSString::from_str(metadata::VERSION);
    let build = NSString::from_str(&metadata::version_label());
    let options = NSMutableDictionary::<NSAboutPanelOptionKey, AnyObject>::new();
    set_about_option(&options, NSAboutPanelOptionApplicationName, &name);
    set_about_option(&options, NSAboutPanelOptionApplicationIcon, &image);
    set_about_option(&options, NSAboutPanelOptionApplicationVersion, &version);
    set_about_option(&options, NSAboutPanelOptionVersion, &build);
    // SAFETY: each key conforms to NSCopying and each value has the documented
    // NSString/NSImage type for the About panel options dictionary.
    unsafe { app.orderFrontStandardAboutPanelWithOptions(&options); }
}

fn set_about_option(
    options: &NSMutableDictionary<NSAboutPanelOptionKey, AnyObject>,
    key: &NSAboutPanelOptionKey,
    value: &impl objc2::Message,
) {
    let key: &ProtocolObject<dyn NSCopying> = ProtocolObject::from_ref(key);
    let value = value as &dyn objc2::Message;
    let value = value as *const dyn objc2::Message as *const AnyObject;
    // SAFETY: call sites pass NSString or NSImage, both Objective-C objects.
    unsafe { options.setObject_forKey(&*value, key); }
}

fn menu_item(mtm: MainThreadMarker, target: &AnyObject, title: &str, key: &str, tag: isize, modifiers: NSEventModifierFlags) -> Retained<NSMenuItem> {
    let item = standard_item(mtm, title, key, sel!(performStudioMenuAction:), modifiers);
    item.setTag(tag);
    // SAFETY: selector is registered on the retained MenuTarget.
    unsafe { item.setTarget(Some(target)); }
    item
}

fn standard_item(mtm: MainThreadMarker, title: &str, key: &str, action: objc2::runtime::Sel, modifiers: NSEventModifierFlags) -> Retained<NSMenuItem> {
    // SAFETY: every selector here has NSMenuItem's one-argument action signature.
    let item = unsafe { NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), &NSString::from_str(title), Some(action), &NSString::from_str(key)) };
    item.setKeyEquivalentModifierMask(modifiers);
    item
}

fn add(menu: &NSMenu, item: Retained<NSMenuItem>) { menu.addItem(&item); }

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
            item.setState(if value { NSControlStateValueOn } else { NSControlStateValueOff });
        }
    }
}

pub(crate) fn update_menu_state(state: MenuState, wants_keyboard_input: bool) {
    let Some(mtm) = MainThreadMarker::new() else { return };
    let app = NSApplication::sharedApplication(mtm);
    let Some(main) = app.mainMenu() else { return };
    let editable = wants_keyboard_input;
    for index in 0..main.numberOfItems() {
        if let Some(item) = main.itemAtIndex(index) {
            if let Some(menu) = item.submenu() {
                checked(&menu, SAVE, state.has_workspace && !state.has_pending_action, None);
                checked(&menu, SAVE_AS, state.has_workspace && !state.has_pending_action, None);
                checked(&menu, CLOSE_WORKSPACE, state.has_workspace && !state.has_pending_action, None);
                checked(&menu, NEW, !state.has_pending_action, None);
                checked(&menu, OPEN, !state.has_pending_action, None);
                checked(&menu, QUIT, !state.has_pending_action, None);
                checked(&menu, SIDEBAR, true, Some(state.show_sidebar));
                checked(&menu, INSPECTOR, true, Some(state.show_inspector));
                checked(&menu, ACTIVITY, true, Some(state.show_activity));
                for tag in [UNDO, REDO, CUT, COPY, PASTE, SELECT_ALL] { checked(&menu, tag, editable, None); }
            }
        }
    }
    let _ = NSControlStateValue::Off;
}
