.pragma library

var activeMenus = [];

function notifyMenuStackChanged(menu) {
    var window = menu ? menu.menuWindow : null;
    if (window && typeof window.contextMenuRevision === "number") {
        window.contextMenuRevision++;
    }
}

function registerMenu(menu) {
    if (activeMenus.indexOf(menu) < 0) {
        activeMenus.push(menu);
        notifyMenuStackChanged(menu);
    }
}

function unregisterMenu(menu) {
    var index = activeMenus.indexOf(menu);
    if (index >= 0) {
        activeMenus.splice(index, 1);
        notifyMenuStackChanged(menu);
    }
}

function hasOpenMenu() {
    for (var i = activeMenus.length - 1; i >= 0; i--) {
        if (activeMenus[i] && activeMenus[i].opened) {
            return true;
        }
    }
    return false;
}

function topMenu() {
    for (var i = activeMenus.length - 1; i >= 0; i--) {
        if (activeMenus[i] && activeMenus[i].opened) {
            return activeMenus[i];
        }
    }
    return null;
}

function closeTopMenu() {
    for (var i = activeMenus.length - 1; i >= 0; i--) {
        var menu = activeMenus[i];
        if (menu && menu.opened) {
            menu.close();
            return true;
        }
    }
    return false;
}

function closeAllMenus() {
    var closedAny = false;
    while (activeMenus.length > 0) {
        var menu = activeMenus.pop();
        if (menu && menu.opened) {
            menu.close();
            closedAny = true;
        }
    }
    return closedAny;
}
