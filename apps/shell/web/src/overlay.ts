// Overlay JS — app switcher (loaded as a module).
// Rust calls renderSwitcher / setSelection / clear via evaluate_javascript,
// which resolves against window globals — we expose them at the bottom.

var switcherEl = document.getElementById('switcher')!;
var dropdownEl = document.getElementById('dropdown')!;
var apps: any[] = [];
var selectedIndex = 0;

// --- App Switcher ---

function renderSwitcher(appList, selected) {
    apps = appList;
    selectedIndex = selected;

    while (switcherEl.firstChild) switcherEl.removeChild(switcherEl.firstChild);
    switcherEl.className = 'active';

    apps.forEach(function(app, i) {
        var el = document.createElement('div');
        el.className = 'app' + (i === selectedIndex ? ' selected' : '');

        var iconEl = document.createElement('div');
        iconEl.className = 'icon';
        iconEl.textContent = '\u2B21';
        el.appendChild(iconEl);

        var nameEl = document.createElement('div');
        nameEl.className = 'name';
        nameEl.textContent = app.name;
        el.appendChild(nameEl);

        el.addEventListener('mouseenter', function() {
            setSelection(i);
        });

        switcherEl.appendChild(el);
    });
}

function setSelection(index) {
    if (index < 0 || index >= apps.length) return;
    selectedIndex = index;
    document.title = String(index);
    var children = switcherEl.children;
    for (var i = 0; i < children.length; i++) {
        children[i].classList.toggle('selected', i === selectedIndex);
    }
}

function clear() {
    apps = [];
    selectedIndex = 0;
    while (switcherEl.firstChild) switcherEl.removeChild(switcherEl.firstChild);
    switcherEl.className = '';
    dropdownEl.className = '';
    while (dropdownEl.firstChild) dropdownEl.removeChild(dropdownEl.firstChild);
}

// --- Dropdown Menus ---

function showDropdown(items, anchorX) {
    while (dropdownEl.firstChild) dropdownEl.removeChild(dropdownEl.firstChild);
    dropdownEl.style.left = anchorX + 'px';
    dropdownEl.className = 'active';

    items.forEach(function(item) {
        if (item.type === 'divider') {
            var div = document.createElement('div');
            div.className = 'menu-divider';
            dropdownEl.appendChild(div);
            return;
        }

        var el = document.createElement('div');
        el.className = 'menu-item' + (item.disabled ? ' disabled' : '');

        var label = document.createElement('span');
        label.textContent = item.label;
        el.appendChild(label);

        if (item.shortcut) {
            var sc = document.createElement('span');
            sc.className = 'shortcut';
            sc.textContent = item.shortcut;
            el.appendChild(sc);
        }

        if (!item.disabled) {
            el.addEventListener('click', function() {
                // Communicate action back via title
                document.title = 'action:' + item.id;
                clear();
            });
        }

        dropdownEl.appendChild(el);
    });
}

// Called from Rust via __solaRecv for bus events forwarded to overlay
// The shell's send_to_js goes to the menubar webview, not here.
// Overlay gets events via evaluate_javascript from Rust directly.

(window as any).renderSwitcher = renderSwitcher;
(window as any).setSelection = setSelection;
(window as any).clear = clear;
