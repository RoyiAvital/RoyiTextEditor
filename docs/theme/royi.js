// mdBook has no option for the search placeholder; it is fixed in its template.
document.getElementById("mdbook-searchbar")?.setAttribute("placeholder", "Search ...");

window.addEventListener("DOMContentLoaded", () => {
	const desktop = window.matchMedia("(min-width: 76.25em)");
	const sidebarToggle = document.getElementById("mdbook-sidebar-toggle-anchor");
	const container = document.getElementById("mdbook-body-container");
	const search = document.getElementById("mdbook-search-wrapper");
	const searchbar = document.getElementById("mdbook-searchbar");
	const searchToggle = document.getElementById("mdbook-search-toggle");
	const menu = document.getElementById("mdbook-menu-bar");
	if (search && searchbar && searchToggle && menu) {
		menu.insertBefore(search, menu.firstChild);
		searchbar.setAttribute("aria-describedby", "mdbook-searchresults-header");
		searchToggle.addEventListener("click", () => {
			const offset = window.scrollY;
			queueMicrotask(() => window.scrollTo(0, offset));
		}, true);
		searchbar.addEventListener("focus", () => {
			if (search.classList.contains("hidden")) {
				const offset = window.scrollY;
				searchToggle.click();
				window.scrollTo(0, offset);
			}
		});
		searchbar.addEventListener("keydown", event => {
			if (event.key !== "Escape") return;
			event.preventDefault();
			event.stopPropagation();
			if (!search.classList.contains("hidden")) searchToggle.click();
			searchbar.blur();
			searchToggle.focus({ preventScroll: true });
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
