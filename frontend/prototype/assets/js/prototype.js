document.addEventListener("DOMContentLoaded", () => {
  if (window.lucide) window.lucide.createIcons();

  const sidebar = document.querySelector("#sidebar");
  document.querySelector("[data-sidebar-open]")?.addEventListener("click", () => sidebar?.classList.add("is-open"));
  document.querySelector("[data-sidebar-close]")?.addEventListener("click", () => sidebar?.classList.remove("is-open"));

  const modal = document.querySelector("[data-modal]");
  const openModal = () => { if (modal) modal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeModal = () => { if (modal) modal.hidden = true; document.body.style.overflow = ""; };
  document.querySelectorAll("[data-modal-open]").forEach((button) => button.addEventListener("click", openModal));
  document.querySelectorAll("[data-modal-close]").forEach((button) => button.addEventListener("click", closeModal));
  modal?.addEventListener("click", (event) => { if (event.target === modal) closeModal(); });
  document.addEventListener("keydown", (event) => { if (event.key === "Escape") closeModal(); });
  document.querySelector("[data-send-form]")?.addEventListener("submit", (event) => {
    event.preventDefault();
    closeModal();
    window.setTimeout(() => window.alert("Test email queued for delivery."), 50);
  });

  const searchInput = document.querySelector("[data-email-search]");
  const emailRows = [...document.querySelectorAll("[data-email-row]")];
  searchInput?.addEventListener("input", () => {
    const query = searchInput.value.trim().toLowerCase();
    emailRows.forEach((row) => { row.hidden = query.length > 0 && !row.dataset.search.includes(query); });
  });
  document.querySelectorAll("[data-filter-button]").forEach((button) => {
    button.addEventListener("click", () => {
      const active = document.querySelector("[data-active-filters]");
      if (active) active.hidden = false;
    });
  });
  document.querySelector(".clear-filters")?.addEventListener("click", () => {
    const active = document.querySelector("[data-active-filters]");
    if (active) active.hidden = true;
  });

  const domainModal = document.querySelector("[data-domain-modal]");
  const openDomainModal = () => { if (domainModal) domainModal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeDomainModal = () => { if (domainModal) domainModal.hidden = true; document.body.style.overflow = ""; };
  document.querySelector("[data-domain-modal-open]")?.addEventListener("click", openDomainModal);
  document.querySelectorAll("[data-domain-modal-close]").forEach((button) => button.addEventListener("click", closeDomainModal));
  domainModal?.addEventListener("click", (event) => { if (event.target === domainModal) closeDomainModal(); });
  document.querySelector("[data-domain-form]")?.addEventListener("submit", (event) => {
    event.preventDefault();
    closeDomainModal();
    window.location.href = "domain-detail.html";
  });
  document.querySelectorAll('a[href="#domains"]').forEach((link) => link.setAttribute("href", "domains.html"));

  const keyModal = document.querySelector("[data-key-modal]");
  const openKeyModal = () => { if (keyModal) keyModal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeKeyModal = () => { if (keyModal) keyModal.hidden = true; document.body.style.overflow = ""; };
  document.querySelector("[data-key-modal-open]")?.addEventListener("click", openKeyModal);
  document.querySelectorAll("[data-key-modal-close]").forEach((button) => button.addEventListener("click", closeKeyModal));
  keyModal?.addEventListener("click", (event) => { if (event.target === keyModal) closeKeyModal(); });
  document.querySelector("[data-key-form]")?.addEventListener("submit", (event) => {
    event.preventDefault();
    closeKeyModal();
    window.setTimeout(() => window.alert("API key created. Copy it now; it will not be shown again."), 50);
  });
  document.querySelectorAll('a[href="#api-keys"]').forEach((link) => link.setAttribute("href", "api-keys.html"));

  const webhookModal = document.querySelector("[data-webhook-modal]");
  const openWebhookModal = () => { if (webhookModal) webhookModal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeWebhookModal = () => { if (webhookModal) webhookModal.hidden = true; document.body.style.overflow = ""; };
  document.querySelector("[data-webhook-modal-open]")?.addEventListener("click", openWebhookModal);
  document.querySelectorAll("[data-webhook-modal-close]").forEach((button) => button.addEventListener("click", closeWebhookModal));
  webhookModal?.addEventListener("click", (event) => { if (event.target === webhookModal) closeWebhookModal(); });
  document.querySelector("[data-webhook-form]")?.addEventListener("submit", (event) => {
    event.preventDefault();
    closeWebhookModal();
    window.setTimeout(() => window.alert("Webhook endpoint created."), 50);
  });
  document.querySelectorAll('a[href="#webhooks"]').forEach((link) => link.setAttribute("href", "webhooks.html"));
  const templateModal = document.querySelector("[data-template-modal]");
  const openTemplateModal = () => { if (templateModal) templateModal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeTemplateModal = () => { if (templateModal) templateModal.hidden = true; document.body.style.overflow = ""; };
  document.querySelector("[data-template-modal-open]")?.addEventListener("click", openTemplateModal);
  document.querySelectorAll("[data-template-modal-close]").forEach((button) => button.addEventListener("click", closeTemplateModal));
  templateModal?.addEventListener("click", (event) => { if (event.target === templateModal) closeTemplateModal(); });
  document.querySelector("[data-template-form]")?.addEventListener("submit", (event) => { event.preventDefault(); closeTemplateModal(); window.location.href = "template-editor.html"; });
  document.querySelectorAll('a[href="#templates"]').forEach((link) => link.setAttribute("href", "templates.html"));
  const suppressionModal = document.querySelector("[data-suppression-modal]");
  const openSuppressionModal = () => { if (suppressionModal) suppressionModal.hidden = false; document.body.style.overflow = "hidden"; };
  const closeSuppressionModal = () => { if (suppressionModal) suppressionModal.hidden = true; document.body.style.overflow = ""; };
  document.querySelector("[data-suppression-modal-open]")?.addEventListener("click", openSuppressionModal);
  document.querySelectorAll("[data-suppression-modal-close]").forEach((button) => button.addEventListener("click", closeSuppressionModal));
  suppressionModal?.addEventListener("click", (event) => { if (event.target === suppressionModal) closeSuppressionModal(); });
  document.querySelector("[data-suppression-form]")?.addEventListener("submit", (event) => { event.preventDefault(); closeSuppressionModal(); window.setTimeout(() => window.alert("Recipient added to suppressions."), 50); });
  document.querySelectorAll('a[href="#suppressions"]').forEach((link) => link.setAttribute("href", "suppressions.html"));
  document.querySelectorAll('a[href="#docs"]').forEach((link) => link.setAttribute("href", "docs.html"));
  document.querySelectorAll('[data-theme-choice]').forEach((button) => button.addEventListener("click", () => {
    const dark = button.dataset.themeChoice === "dark";
    document.body.classList.toggle("theme-dark", dark);
    document.querySelectorAll('[data-theme-choice]').forEach((choice) => choice.classList.toggle("theme-choice--active", choice === button));
  }));
  document.querySelectorAll('a[href="#team"], a[href="#billing"], a[href="#settings"]').forEach((link) => {
    if (link.getAttribute("href") === "#team") link.setAttribute("href", "settings.html#team");
    if (link.getAttribute("href") === "#billing") link.setAttribute("href", "billing.html");
    if (link.getAttribute("href") === "#settings") link.setAttribute("href", "settings.html");
  });
  document.querySelectorAll(".auth-form, .onboarding-form").forEach((form) => form.addEventListener("submit", (event) => {
    event.preventDefault();
    const destination = form.closest(".onboarding-main") ? "domains.html" : "onboarding.html";
    window.location.href = destination;
  }));
  const drawer = document.querySelector("[data-notification-drawer]");
  const openDrawer = () => { drawer?.classList.add("is-open"); drawer?.setAttribute("aria-hidden", "false"); };
  const closeDrawer = () => { drawer?.classList.remove("is-open"); drawer?.setAttribute("aria-hidden", "true"); };
  document.querySelector("[data-notifications-open]")?.addEventListener("click", openDrawer);
  document.querySelector("[data-notifications-close]")?.addEventListener("click", closeDrawer);
  document.addEventListener("keydown", (event) => { if (event.key === "Escape") closeDrawer(); });
  const toast = document.querySelector("[data-toast]");
  const showToast = () => { if (!toast) return; toast.hidden = false; window.setTimeout(() => { toast.hidden = true; }, 3500); };
  document.querySelector("[data-toast-trigger]")?.addEventListener("click", showToast);
  document.querySelector("[data-toast-close]")?.addEventListener("click", () => { if (toast) toast.hidden = true; });
  document.querySelectorAll('[aria-label^="Copy"]').forEach((button) => button.addEventListener("click", () => showToast()));

  document.querySelectorAll(".nav").forEach((nav) => {
    const accountLabel = [...nav.querySelectorAll(".nav__label")].find((label) => label.textContent.trim().toLowerCase() === "account");
    if (accountLabel) {
      let current = accountLabel;
      while (current) {
        const next = current.nextElementSibling;
        current.remove();
        if (!next || next.classList.contains("nav__label")) break;
        current = next;
      }
    }
  });

  if (window.location.pathname.endsWith("/settings.html")) {
    document.querySelector("#billing")?.remove();
    document.querySelector('.settings-nav a[href="billing.html"], .settings-nav a[href="#billing"]')?.remove();
  }

  document.querySelectorAll(".profile-link").forEach((profileLink) => {
    profileLink.removeAttribute("href");
    profileLink.setAttribute("role", "button");
    profileLink.setAttribute("tabindex", "0");
    profileLink.setAttribute("aria-haspopup", "menu");
    profileLink.setAttribute("aria-expanded", "false");
    const profileChevron = profileLink.querySelector(":scope > svg");
    if (profileChevron) profileChevron.outerHTML = '<i class="profile-chevron" data-lucide="chevron-up"></i>';
    const menu = document.createElement("div");
    menu.className = "profile-menu";
    menu.setAttribute("role", "menu");
    menu.innerHTML = '<div class="profile-menu__identity"><strong>Alex Morgan</strong><small>alex@acme.dev</small></div><a role="menuitem" href="settings.html"><i data-lucide="settings-2"></i><span>Settings</span></a><a role="menuitem" href="billing.html"><i data-lucide="credit-card"></i><span>Billing</span></a><div class="profile-menu__divider"></div><a class="profile-menu__signout" role="menuitem" href="login.html"><i data-lucide="log-out"></i><span>Sign out</span></a>';
    profileLink.appendChild(menu);
    const toggleProfile = (event) => {
      event.preventDefault();
      event.stopPropagation();
      const isOpen = profileLink.classList.toggle("is-open");
      profileLink.setAttribute("aria-expanded", String(isOpen));
    };
    profileLink.addEventListener("click", toggleProfile);
    profileLink.addEventListener("keydown", (event) => { if (event.key === "Enter" || event.key === " ") toggleProfile(event); });
    menu.addEventListener("click", (event) => event.stopPropagation());
  });
  document.addEventListener("click", () => document.querySelectorAll(".profile-link.is-open").forEach((link) => link.classList.remove("is-open")));
  if (window.lucide) window.lucide.createIcons();
});
