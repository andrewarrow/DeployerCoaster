// Interact only with the Cloudflare web area in the foreground browser.
// Native keystrokes preserve the normal browser session and input behavior.
function prepareCloudflareLogin(se, process, credentials, pause = delay) {
    function attribute(element, name) {
        try { return element.attributes.byName(name).value(); }
        catch (_) { return ''; }
    }
    function label(element) {
        for (const name of ['AXTitle', 'AXDescription', 'AXHelp']) {
            const value = attribute(element, name);
            if (typeof value === 'string' && value) return value.trim();
        }
        return '';
    }
    function loginArea() {
        if (!process.frontmost() || !process.windows.length) return null;
        const window = process.windows[0];
        for (const element of window.entireContents()) {
            if (attribute(element, 'AXRole') !== 'AXWebArea') continue;
            const url = attribute(element, 'AXURL') || attribute(element, 'AXDocument')
                || attribute(window, 'AXDocument');
            if (typeof url === 'string' && /^https:\/\/dash\.cloudflare\.com\/login(?:[?#]|$)/.test(url)) {
                return element;
            }
        }
        return null;
    }
    function typeInto(element, text) {
        if (!process.frontmost() || !loginArea()) return false;
        element.focused = true;
        if (!element.focused()) return false;
        se.keystroke('a', {using: 'command down'});
        se.keystroke(text);
        return true;
    }
    let selectedAnotherProfile = false;
    for (let attempt = 0; attempt < 80; attempt++) {
        pause(0.5);
        if (!process.frontmost()) return 'focus';
        const area = loginArea();
        if (!area) continue;
        const elements = area.entireContents();
        if (!selectedAnotherProfile) {
            const otherProfile = elements.find(element =>
                attribute(element, 'AXRole') === 'AXButton'
                && label(element) === 'Sign in with another profile');
            if (otherProfile) {
                otherProfile.click();
                selectedAnotherProfile = true;
                continue;
            }
        }
        const email = elements.find(element =>
            attribute(element, 'AXRole') === 'AXTextField'
            && label(element).toLowerCase() === 'email');
        const password = elements.find(element =>
            attribute(element, 'AXRole') === 'AXTextField'
            && (attribute(element, 'AXSubrole') === 'AXSecureTextField'
                || label(element).toLowerCase() === 'password'));
        if (!email || !password) continue;
        if (!typeInto(email, credentials[0])) return 'focus';
        if (!typeInto(password, credentials[1])) return 'focus';
        return 'filled';
    }
    return 'unavailable';
}
