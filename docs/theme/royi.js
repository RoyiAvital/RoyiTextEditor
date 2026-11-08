// mdBook has no option for the search placeholder; it is fixed in its template.
document.getElementById("mdbook-searchbar")?.setAttribute("placeholder", "Search ...");
// The sidebar heading shows the book title (book.title or MDBOOK_BOOK__TITLE).
const bookTitle = document.querySelector(".menu-title")?.textContent.trim();
if (bookTitle) {
	document.querySelector(".sidebar .sidebar-scrollbox")?.setAttribute("data-title", bookTitle);
}
// The header logo is the favicon, whose file name mdBook fingerprints.
const favicon = document.querySelector('link[rel="icon"]');
if (favicon) {
	document.documentElement.style.setProperty("--logo", `url("${favicon.href}")`);
}

window.addEventListener("DOMContentLoaded", () => {
	const desktop = window.matchMedia("(min-width: 76.25em)");
	const sidebarToggle = document.getElementById("mdbook-sidebar-toggle-anchor");
	const container = document.getElementById("mdbook-body-container");
	const search = document.getElementById("mdbook-search-wrapper");
	const searchbar = document.getElementById("mdbook-searchbar");
	const searchToggle = document.getElementById("mdbook-search-toggle");
	const menu = document.getElementById("mdbook-menu-bar");
	const results = document.getElementById("mdbook-searchresults");
	if (search && searchbar && searchToggle && menu && results) {
		// The search box is in the header. It is open (mdBook's `hidden` class
		// removed) exactly while the focus is in it: focusing it opens it; Esc
		// or the focus moving elsewhere closes it. Its width and results follow
		// the open state (royi.css).
		menu.insertBefore(search, menu.firstChild);
		searchbar.setAttribute("aria-describedby", "mdbook-searchresults-header");
		const isOpen = () => !search.classList.contains("hidden");
		const focusSearchbar = () => {
			if (isOpen() && !search.contains(document.activeElement)) {
				searchbar.focus({ preventScroll: true });
			}
		};
		// mdBook scrolls to the top when the search opens (its box was at the
		// top of the page); the header box keeps the reading position.
		let scrollBefore = 0;
		for (const type of ["click", "keydown"]) {
			window.addEventListener(type, () => { scrollBefore = window.scrollY; }, true);
			window.addEventListener(type, () => {
				if (isOpen() && window.scrollY !== scrollBefore) {
					window.scrollTo({ top: scrollBefore, behavior: "instant" });
				}
			});
		}
		searchbar.addEventListener("focus", () => {
			if (!isOpen()) searchToggle.click();
		});
		// A search opened from the address (`?search=`) takes the focus.
		new MutationObserver(focusSearchbar).observe(search, { attributeFilter: ["class"] });
		focusSearchbar();
		// Closing leaves no search in the address, so a reload does not reopen it.
		const closeSearch = () => {
			if (isOpen()) searchToggle.click();
			const url = new URL(window.location.href);
			if (url.searchParams.has("search")) {
				url.searchParams.delete("search");
				history.replaceState(history.state, "", url);
			}
		};
		// Focus leaving the box closes it; another window taking the focus does not.
		search.addEventListener("focusout", () => {
			window.setTimeout(() => {
				if (document.hasFocus() && !search.contains(document.activeElement)) closeSearch();
			});
		});
		// Clicks on the results keep the focus in the field (a result link
		// still opens on click).
		search.addEventListener("mousedown", event => {
			if (event.button === 0 && event.target !== searchbar) event.preventDefault();
		});
		// The field's clear button changes the text without a key.
		searchbar.addEventListener("input", () => {
			if (searchbar.value === "") searchbar.dispatchEvent(new KeyboardEvent("keyup"));
		});
		// Esc closes; Down and Up move between the field and the results; Enter
		// in the field opens the first result. mdBook's own handling moves the
		// focus out of the box.
		search.addEventListener("keydown", event => {
			if (event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return;
			const links = [...results.querySelectorAll("li > a")];
			const at = links.indexOf(document.activeElement);
			// The selected result is marked as mdBook does (`li.focus`).
			const select = target => {
				for (const link of links) link.parentElement.classList.toggle("focus", link === target);
				target?.focus();
			};
			if (event.key === "Escape") {
				closeSearch();
				if (desktop.matches) {
					document.activeElement?.blur();
				} else {
					searchToggle.focus({ preventScroll: true });
				}
			} else if (event.key === "ArrowDown") {
				if (links[at + 1]) select(links[at + 1]);
			} else if (event.key === "ArrowUp" && at >= 0) {
				select(links[at - 1] || searchbar);
			} else if (event.key === "Enter" && event.target === searchbar && links.length) {
				window.location.assign(links[0].href);
			} else {
				return;
			}
			event.preventDefault();
			event.stopPropagation();
		}, true);
	}
	if (sidebarToggle) {
		const syncSidebar = () => {
			if (sidebarToggle.checked !== desktop.matches) {
				(document.getElementById("mdbook-sidebar-toggle") || sidebarToggle).click();
			}
		};
		syncSidebar();
		desktop.addEventListener("change", syncSidebar);
		const overlay = document.createElement("label");
		overlay.className = "sidebar-overlay";
		overlay.htmlFor = sidebarToggle.id;
		overlay.setAttribute("aria-hidden", "true");
		container?.append(overlay);
		document.addEventListener("keydown", event => {
			if (event.key === "Escape" && !desktop.matches && sidebarToggle.checked) {
				sidebarToggle.click();
				document.getElementById("mdbook-sidebar-toggle")?.focus();
			}
		});
	}

	const chapters = [...document.querySelectorAll(".sidebar .chapter a")];
	const navigation = document.createElement("nav");
	navigation.className = "chapter-footer__inner";
	navigation.setAttribute("aria-label", "Chapter navigation");
	for (const [relation, direction] of [["prev", "Previous"], ["next", "Next"]]) {
		const source = document.querySelector(`.nav-wide-wrapper a[rel~="${relation}"]`);
		if (!source) continue;
		const name = chapters.find(chapter => chapter.href === source.href)?.textContent.trim()
			|| direction;
		const link = source.cloneNode(false);
		link.className = `chapter-footer__link chapter-footer__link--${relation}`;
		link.setAttribute("aria-label", `${direction}: ${name}`);
		link.title = `${direction}: ${name}`;
		const title = document.createElement("span");
		title.className = "chapter-footer__title";
		const label = document.createElement("span");
		label.className = "chapter-footer__direction";
		label.textContent = direction;
		title.append(label, document.createTextNode(name));
		const icon = source.querySelector(".fa-svg")?.cloneNode(true);
		if (icon) {
			icon.className = "chapter-footer__icon";
			icon.setAttribute("aria-hidden", "true");
		}
		if (relation === "prev" && icon) link.append(icon);
		link.append(title);
		if (relation === "next" && icon) link.append(icon);
		navigation.append(link);
	}
	if (navigation.childElementCount) {
		const footer = document.createElement("footer");
		footer.className = "chapter-footer";
		footer.append(navigation);
		container?.append(footer);
	}
}, { once: true });
