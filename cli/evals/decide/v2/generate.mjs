#!/usr/bin/env node
// Deterministic generator for the v2 decision eval set.
//
// Every fixture is rendered from the element list below, in declaration order.
// Every case names the operation, the expected accessible name, and the values
// the caller supplies. The labels were written from those element lists and
// fixture construction before any decider call, and are never derived from
// model output.
//
// Run from anywhere:
//   node cli/evals/decide/v2/generate.mjs
//
// The seed is fixed and recorded for reproducibility. Fixture text and labels
// contain no randomness, so output is byte stable.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SEED = 20261108;
const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(HERE, "fixtures");

const slug = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

let nextId = 0;

const html = (elements, indent = "      ") =>
  elements.map((element) => element.render(indent)).join("\n");

function labelled(control, name, attributes = "") {
  nextId += 1;
  const id = `f${nextId}`;
  return {
    name,
    render(indent) {
      return `${indent}<label for="${id}">${name}</label>\n${indent}${control(id, attributes)}`;
    },
  };
}

const elements = {
  nav(links) {
    return {
      names: links.map((link) => link.name),
      render(indent) {
        return `${indent}<nav>\n${links
          .map((link) => `${indent}  <a href="${link.href}">${link.name}</a>`)
          .join("\n")}\n${indent}</nav>`;
      },
    };
  },
  link(name, href = "/") {
    return {
      name,
      render(indent) {
        return `${indent}<a href="${href}">${name}</a>`;
      },
    };
  },
  button(name, options = {}) {
    return {
      name,
      render(indent) {
        const attributes = [];
        if (options.role) attributes.push(`role="${options.role}"`);
        if (options.disabled) attributes.push("disabled");
        if (options.ariaDisabled) attributes.push('aria-disabled="true"');
        if (options.type) attributes.push(`type="${options.type}"`);
        const suffix = attributes.length ? ` ${attributes.join(" ")}` : "";
        return `${indent}<button${suffix}>${name}</button>`;
      },
    };
  },
  input(name, options = {}) {
    return labelled(
      (id, extra) =>
        `<input id="${id}" type="${options.type || "text"}"${extra}>`,
      name,
      options.placeholder ? ` placeholder="${options.placeholder}"` : "",
    );
  },
  textarea(name) {
    return labelled((id) => `<textarea id="${id}" rows="3"></textarea>`, name);
  },
  select(name, options) {
    return labelled(
      (id) =>
        `<select id="${id}">\n${options
          .map((option) => `          <option>${option}</option>`)
          .join("\n")}\n        </select>`,
      name,
    );
  },
  checkbox(name, options = {}) {
    nextId += 1;
    const id = `f${nextId}`;
    return {
      name,
      render(indent) {
        const disabled = options.disabled ? " disabled" : "";
        return `${indent}<input id="${id}" type="checkbox"${disabled}>\n${indent}<label for="${id}">${name}</label>`;
      },
    };
  },
  radio(name, group = "choice") {
    nextId += 1;
    const id = `f${nextId}`;
    return {
      name,
      render(indent) {
        return `${indent}<input id="${id}" type="radio" name="${group}">\n${indent}<label for="${id}">${name}</label>`;
      },
    };
  },
  heading(level, text) {
    return {
      render(indent) {
        return `${indent}<h${level}>${text}</h${level}>`;
      },
    };
  },
  paragraph(text) {
    return {
      render(indent) {
        return `${indent}<p>${text}</p>`;
      },
    };
  },
  raw(markup) {
    return {
      render(indent) {
        return `${indent}${markup}`;
      },
    };
  },
  spacer(height) {
    return {
      render(indent) {
        return `${indent}<div style="height: ${height}px"> </div>`;
      },
    };
  },
  section(title, children) {
    return {
      children,
      render(indent) {
        return `${indent}<section>\n${indent}  <h2>${title}</h2>\n${html(
          children,
          `${indent}  `,
        )}\n${indent}</section>`;
      },
    };
  },
  form(children) {
    return {
      children,
      render(indent) {
        return `${indent}<form>\n${html(children, `${indent}  `)}\n${indent}</form>`;
      },
    };
  },
  article(children) {
    return {
      children,
      render(indent) {
        return `${indent}<article>\n${html(children, `${indent}  `)}\n${indent}</article>`;
      },
    };
  },
};

function page(request) {
  return { ...request, elements: request.build() };
}

function renderPage(spec) {
  const body = spec.elements.map((element) => element.render("    ")).join("\n");
  return `<!doctype html>
<html lang="en">
  <head>
    <meta charset="utf-8">
    <meta name="viewport" content="width=device-width, initial-scale=1">
    <title>${spec.title}</title>
  </head>
  <body>
${body}
  </body>
</html>
`;
}

function targetCase(pageId, operation, target, instruction, values, suffix) {
  const entry = {
    name: `${pageId}-${operation}-${slug(target)}${suffix ? `-${suffix}` : ""}`,
    fixture: `${pageId}.html`,
    instruction,
  };
  if (values) entry.values = values;
  entry.expect = { operation, target_name: target };
  return entry;
}

function plainCase(pageId, operation, instruction, suffix) {
  return {
    name: suffix ? `${pageId}-${operation}-${suffix}` : `${pageId}-${operation}`,
    fixture: `${pageId}.html`,
    instruction,
    expect: { operation },
  };
}

function blockedCase(pageId, operation, target, instruction) {
  return {
    name: `${pageId}-${operation}-${slug(target)}-blocked`,
    fixture: `${pageId}.html`,
    instruction,
    expect: { operation, target_name: target, blocked: true },
  };
}

function click(pageId, target, instruction) {
  return targetCase(pageId, "click", target, instruction);
}

function fill(pageId, target, instruction, key, value, suffix) {
  return targetCase(pageId, "fill", target, instruction, { [key]: value }, suffix);
}

function select(pageId, target, instruction, key, value) {
  return targetCase(pageId, "select", target, instruction, { [key]: value });
}

function blockedFill(pageId, target, instruction) {
  return blockedCase(pageId, "fill", target, instruction);
}

function blockedSelect(pageId, target, instruction) {
  return blockedCase(pageId, "select", target, instruction);
}

function scrollDown(pageId, instruction, suffix) {
  return plainCase(pageId, "scroll_down", instruction, suffix);
}

function scrollUp(pageId, instruction, suffix) {
  return plainCase(pageId, "scroll_up", instruction, suffix);
}

function historyBack(pageId, instruction, suffix) {
  return plainCase(pageId, "back", instruction, suffix);
}

function wait(pageId, instruction, suffix) {
  return plainCase(pageId, "wait", instruction, suffix);
}

const pages = [];

// 1. Authentication portal.
pages.push(
  page({
    id: "auth_portal",
    title: "Northwind sign in",
    build: () => [
      elements.nav([
        { name: "Home", href: "/" },
        { name: "Help", href: "/help" },
      ]),
      elements.heading(1, "Sign in to Northwind"),
      elements.paragraph("Use your workspace account to continue."),
      elements.form([
        elements.input("Email address", { type: "email", placeholder: "you@example.com" }),
        elements.input("Password", { type: "password" }),
        elements.checkbox("Remember this device"),
        elements.select("Region", ["United States", "Germany", "Japan"]),
        elements.select("Language", ["English", "German", "Japanese"]),
        elements.select("Time zone display", ["UTC", "Local time"]),
        elements.button("Sign in", { type: "submit" }),
      ]),
      elements.link("Create an account", "/register"),
      elements.link("Forgot password", "/reset"),
      elements.button("Continue with SSO"),
      elements.link("Use a passkey", "/passkey"),
      elements.link("Privacy policy", "/privacy"),
      elements.spacer(1800),
      elements.link("Security notice", "/security"),
    ],
    cases: [
      click("auth_portal", "Help", "Open Help for account recovery questions"),
      click("auth_portal", "Sign in", "Sign in with the saved workspace account"),
      click("auth_portal", "Create an account", "Create an account for a new teammate"),
      click("auth_portal", "Forgot password", "Forgot password, so start the reset flow"),
      click("auth_portal", "Continue with SSO", "Continue with SSO through the company identity provider"),
      click("auth_portal", "Remember this device", "Turn on Remember this device for this browser"),
      click("auth_portal", "Use a passkey", "Use a passkey instead of the typed password"),
      fill("auth_portal", "Email address", "Put the work email into the Email address field", "email", "harbor@example.com"),
      fill("auth_portal", "Email address", "Use the backup address in Email address", "email", "ops@example.com", "backup"),
      fill("auth_portal", "Password", "Type the account secret into the Password field", "password", "long example passphrase"),
      fill("auth_portal", "Password", "Enter the backup secret in Password", "password", "second example passphrase", "backup"),
      blockedFill("auth_portal", "Email address", "Fill the Email address field for this login"),
      select("auth_portal", "Region", "Set Region to Germany for this session", "region", "Germany"),
      select("auth_portal", "Language", "Switch Language to Japanese", "language", "Japanese"),
      select("auth_portal", "Time zone display", "Choose Time zone display Local time", "time_zone", "Local time"),
      blockedSelect("auth_portal", "Region", "Pick the right Region before continuing"),
      scrollDown("auth_portal", "Scroll down to the security notice at the bottom"),
      scrollUp("auth_portal", "Scroll up to the account form"),
      historyBack("auth_portal", "Take me back to the site I visited before this login page"),
      wait("auth_portal", "Wait for the single sign-on prompt to appear"),
      scrollDown("auth_portal", "Scroll down through the account policy text", "policy"),
      scrollUp("auth_portal", "Scroll up toward the Northwind heading", "heading"),
    ],
  }),
);

// 2. Commerce catalog.
pages.push(
  page({
    id: "shop_catalog",
    title: "Harbor Supply catalog",
    build: () => [
      elements.nav([
        { name: "Orders", href: "/orders" },
        { name: "Account", href: "/account" },
        { name: "Wishlist", href: "/wishlist" },
        { name: "Cart", href: "/cart" },
      ]),
      elements.heading(1, "Harbor Supply"),
      elements.input("Search products", { type: "search" }),
      elements.button("Search"),
      elements.select("Category", ["All departments", "Footwear", "Jackets", "Packs"]),
      elements.select("Sort by", ["Featured", "Price low to high", "Newest"]),
      elements.select("Currency", ["USD", "EUR", "GBP"]),
      elements.checkbox("In stock only"),
      elements.button("Apply filters"),
      elements.button("Clear filters"),
      elements.section("Trail collection", [
        elements.link("Ridge Runner 2", "/ridge-runner"),
        elements.link("View details", "/ridge-runner"),
        elements.button("Add to cart"),
        elements.link("Mudline GTX", "/mudline"),
        elements.link("View details", "/mudline"),
        elements.button("Add to cart"),
        elements.link("Alpine Shell", "/alpine-shell"),
      ]),
      elements.section("Pagination", [
        elements.button("Previous page"),
        elements.button("Next page"),
      ]),
      elements.input("Coupon code"),
      elements.spacer(2200),
    ],
    cases: [
      click("shop_catalog", "Cart", "Open Cart to review the selected items"),
      click("shop_catalog", "Orders", "Open Orders to track a recent purchase"),
      click("shop_catalog", "Search", "Run Search for the typed product name"),
      click("shop_catalog", "Apply filters", "Apply filters to the product grid"),
      click("shop_catalog", "Clear filters", "Clear filters and show every department"),
      click("shop_catalog", "In stock only", "Turn on In stock only for the listing"),
      click("shop_catalog", "Ridge Runner 2", "Open Ridge Runner 2 and read the specification"),
      fill("shop_catalog", "Search products", "Enter trail shoes in Search products", "query", "trail shoes"),
      fill("shop_catalog", "Search products", "Search for hiking poles in Search products", "query", "hiking poles", "poles"),
      fill("shop_catalog", "Coupon code", "Use the code HARBOR15 in Coupon code", "coupon", "HARBOR15"),
      fill("shop_catalog", "Coupon code", "Try code SPRING20 in Coupon code", "coupon", "SPRING20", "spring"),
      blockedFill("shop_catalog", "Search products", "Fill Search products with the item I want"),
      select("shop_catalog", "Category", "Set Category to Footwear", "category", "Footwear"),
      select("shop_catalog", "Sort by", "Choose Sort by Price low to high", "sort", "Price low to high"),
      select("shop_catalog", "Currency", "Change Currency to GBP", "currency", "GBP"),
      blockedSelect("shop_catalog", "Sort by", "Select the newest products with Sort by"),
      scrollDown("shop_catalog", "Scroll down to the second row of products"),
      scrollUp("shop_catalog", "Scroll up to the store header"),
      historyBack("shop_catalog", "Take me back to the page I came from"),
      wait("shop_catalog", "Wait for the deals carousel to load"),
      historyBack("shop_catalog", "Return to the category page that preceded this listing", "category"),
      wait("shop_catalog", "Wait until the filter counts become ready", "filter-counts"),
    ],
  }),
);

// 3. Checkout.
pages.push(
  page({
    id: "checkout_flow",
    title: "Checkout",
    build: () => [
      elements.nav([
        { name: "Home", href: "/" },
        { name: "Cart", href: "/cart" },
      ]),
      elements.heading(1, "Checkout"),
      elements.form([
        elements.input("Full name"),
        elements.input("Street address"),
        elements.input("City"),
        elements.select("Country", ["United States", "Canada", "Germany"]),
        elements.select("Delivery speed", ["Standard", "Express"]),
        elements.select("Payment method", ["Card", "PayPal", "Bank transfer"]),
        elements.input("Coupon code"),
        elements.button("Apply coupon"),
        elements.checkbox("Same as billing"),
        elements.button("Place order"),
      ]),
      elements.link("Edit payment method", "/payment"),
      elements.link("Contact support", "/support"),
      elements.link("Return to cart", "/cart"),
      elements.link("View shipping policy", "/shipping"),
      elements.spacer(2000),
    ],
    cases: [
      click("checkout_flow", "Apply coupon", "Apply coupon to the current basket"),
      click("checkout_flow", "Same as billing", "Use Same as billing for the delivery address"),
      click("checkout_flow", "Place order", "Place order and pay for the basket"),
      click("checkout_flow", "Edit payment method", "Edit payment method before the charge"),
      click("checkout_flow", "Contact support", "Contact support about a delivery problem"),
      click("checkout_flow", "Return to cart", "Return to cart and change the quantity"),
      fill("checkout_flow", "Full name", "Put the recipient name in Full name", "full_name", "Riley Chen"),
      fill("checkout_flow", "Full name", "Use the gift recipient in Full name", "full_name", "Alex Rivera", "gift"),
      fill("checkout_flow", "Street address", "Enter the street in Street address", "street", "18 Harbor Road"),
      fill("checkout_flow", "City", "Fill City with Portland", "city", "Portland"),
      fill("checkout_flow", "City", "Set City to Seattle", "city", "Seattle", "alternate"),
      blockedFill("checkout_flow", "Full name", "Fill Full name for the shipping label"),
      select("checkout_flow", "Country", "Set Country to Canada", "country", "Canada"),
      select("checkout_flow", "Delivery speed", "Choose Delivery speed Express", "speed", "Express"),
      select("checkout_flow", "Payment method", "Pay with Payment method PayPal", "payment", "PayPal"),
      blockedSelect("checkout_flow", "Delivery speed", "Select the faster Delivery speed option"),
      scrollDown("checkout_flow", "Scroll down to the order summary"),
      scrollUp("checkout_flow", "Scroll up to the top of the checkout form"),
      historyBack("checkout_flow", "Go to the screen before this checkout"),
      wait("checkout_flow", "Wait for the tax estimate to settle"),
      historyBack("checkout_flow", "Go back to the cart from before this checkout", "cart"),
      wait("checkout_flow", "Wait until the delivery estimate finishes loading", "delivery"),
    ],
  }),
);

// 4. Admin settings.
pages.push(
  page({
    id: "admin_settings",
    title: "Workspace settings",
    build: () => [
      elements.heading(1, "Workspace settings"),
      elements.button("General", { role: "tab" }),
      elements.button("Members", { role: "tab" }),
      elements.button("Billing", { role: "tab" }),
      elements.button("Integrations", { role: "tab" }),
      elements.link("Docs", "/docs"),
      elements.input("Workspace name"),
      elements.input("Invite email", { type: "email" }),
      elements.input("Search members", { type: "search" }),
      elements.select("Time zone", ["UTC", "Europe/Berlin", "America/New_York"]),
      elements.select("Default role", ["Admin", "Editor", "Viewer"]),
      elements.select("Week starts on", ["Monday", "Sunday"]),
      elements.checkbox("Require two-factor authentication"),
      elements.checkbox("Allow guest access", { disabled: true }),
      elements.button("Save changes"),
      elements.button("Discard"),
      elements.button("Send invite"),
      elements.button("Delete workspace", { ariaDisabled: true }),
      elements.link("View audit log", "/audit"),
      elements.link("Manage API keys", "/api-keys"),
      elements.spacer(2400),
    ],
    cases: [
      click("admin_settings", "General", "Open the General tab of workspace settings"),
      click("admin_settings", "Members", "Open Members to see everyone in the workspace"),
      click("admin_settings", "Integrations", "Open Integrations and connect a tool"),
      click("admin_settings", "Save changes", "Save changes to the workspace profile"),
      click("admin_settings", "Discard", "Discard the unsaved workspace edits"),
      click("admin_settings", "Send invite", "Send invite to the typed email address"),
      click("admin_settings", "View audit log", "Open the View audit log for recent changes"),
      fill("admin_settings", "Workspace name", "Rename the Workspace name to Harbor Labs", "name", "Harbor Labs"),
      fill("admin_settings", "Workspace name", "Use Northwind Studio as the Workspace name", "name", "Northwind Studio", "alternate"),
      fill("admin_settings", "Invite email", "Put the new member address in Invite email", "email", "new.member@example.com"),
      fill("admin_settings", "Invite email", "Invite the contractor through Invite email", "email", "contractor@example.com", "contractor"),
      blockedFill("admin_settings", "Workspace name", "Fill Workspace name with the new company name"),
      select("admin_settings", "Time zone", "Set Time zone to Europe/Berlin", "time_zone", "Europe/Berlin"),
      select("admin_settings", "Default role", "Make Default role Viewer", "role", "Viewer"),
      select("admin_settings", "Week starts on", "Choose Week starts on Sunday", "week_start", "Sunday"),
      blockedSelect("admin_settings", "Default role", "Select a different Default role for invites"),
      scrollDown("admin_settings", "Scroll down to the audit log section"),
      scrollUp("admin_settings", "Scroll up to the tab bar"),
      historyBack("admin_settings", "Go back to the settings page I opened before"),
      wait("admin_settings", "Wait until the member list has loaded"),
      scrollDown("admin_settings", "Scroll down to the danger zone", "danger"),
      scrollUp("admin_settings", "Scroll up toward the workspace header", "header"),
    ],
  }),
);

// 5. Documentation search.
pages.push(
  page({
    id: "docs_search",
    title: "Platform documentation",
    build: () => [
      elements.nav([
        { name: "Getting started", href: "/start" },
        { name: "API reference", href: "/api" },
        { name: "Changelog", href: "/changelog" },
        { name: "Support", href: "/support" },
      ]),
      elements.heading(1, "Platform documentation"),
      elements.input("Search documentation", { type: "search" }),
      elements.button("Search"),
      elements.input("Filter pages", { type: "search" }),
      elements.select("Version", ["Latest", "v2", "v1"]),
      elements.select("Code language", ["Bash", "Python", "JavaScript"]),
      elements.select("Theme", ["System", "Light", "Dark"]),
      elements.link("Installation", "/install"),
      elements.link("Configuration", "/config"),
      elements.link("Troubleshooting", "/troubleshooting"),
      elements.button("Copy code"),
      elements.button("Previous article"),
      elements.button("Next article"),
      elements.link("Edit this page", "/edit"),
      elements.link("Back to guides", "/guides"),
      elements.spacer(2600),
    ],
    cases: [
      click("docs_search", "Search", "Run Search for the documentation query"),
      click("docs_search", "Getting started", "Open Getting started for a new install"),
      click("docs_search", "API reference", "Go to the API reference for endpoint details"),
      click("docs_search", "Configuration", "Open Configuration for environment options"),
      click("docs_search", "Troubleshooting", "Open Troubleshooting for a failing request"),
      click("docs_search", "Copy code", "Copy code from the current example"),
      click("docs_search", "Next article", "Next article in the guide sequence"),
      fill("docs_search", "Search documentation", "Type webhook retries into Search documentation", "query", "webhook retries"),
      fill("docs_search", "Search documentation", "Look up rate limits in Search documentation", "query", "rate limits", "limits"),
      fill("docs_search", "Filter pages", "Put migration into Filter pages", "filter", "migration"),
      fill("docs_search", "Filter pages", "Use sdk in Filter pages", "filter", "sdk", "sdk"),
      blockedFill("docs_search", "Search documentation", "Fill Search documentation with the API name"),
      select("docs_search", "Version", "Set Version to v2", "version", "v2"),
      select("docs_search", "Code language", "Use Code language Python for the samples", "language", "Python"),
      select("docs_search", "Theme", "Change Theme to Dark", "theme", "Dark"),
      blockedSelect("docs_search", "Version", "Select the older Version of the docs"),
      scrollDown("docs_search", "Scroll down through the setup guide"),
      scrollUp("docs_search", "Scroll up to the API reference summary"),
      historyBack("docs_search", "Return to the article I opened before this one"),
      wait("docs_search", "Wait for the search results to appear"),
    ],
  }),
);

// 6. Travel search.
pages.push(
  page({
    id: "travel_search",
    title: "Waypoint travel",
    build: () => [
      elements.nav([
        { name: "Manage booking", href: "/booking" },
        { name: "Sign in", href: "/signin" },
        { name: "Deals", href: "/deals" },
      ]),
      elements.heading(1, "Find a flight"),
      elements.form([
        elements.input("Origin", { placeholder: "City or airport" }),
        elements.input("Destination", { placeholder: "City or airport" }),
        elements.input("Depart date", { placeholder: "YYYY-MM-DD" }),
        elements.input("Promo code"),
        elements.select("Passengers", ["1", "2", "3", "4"]),
        elements.select("Cabin", ["Economy", "Premium", "Business"]),
        elements.select("Stops", ["Any", "Nonstop", "One stop"]),
        elements.checkbox("Direct flights only"),
        elements.checkbox("Flexible dates"),
        elements.button("Search flights"),
      ]),
      elements.button("Next month"),
      elements.button("Previous month"),
      elements.link("Travel alerts", "/alerts"),
      elements.spacer(2400),
    ],
    cases: [
      click("travel_search", "Manage booking", "Open Manage booking for an existing trip"),
      click("travel_search", "Direct flights only", "Turn on Direct flights only"),
      click("travel_search", "Flexible dates", "Enable Flexible dates for the return trip"),
      click("travel_search", "Search flights", "Search flights with the filled route"),
      click("travel_search", "Next month", "Show Next month on the fare calendar"),
      click("travel_search", "Previous month", "Go to Previous month on the fare calendar"),
      fill("travel_search", "Origin", "Put Seattle in Origin", "origin", "Seattle"),
      fill("travel_search", "Origin", "Use Portland in Origin", "origin", "Portland", "portland"),
      fill("travel_search", "Destination", "Enter Tokyo in Destination", "destination", "Tokyo"),
      fill("travel_search", "Destination", "Fly to Seoul in Destination", "destination", "Seoul", "seoul"),
      fill("travel_search", "Depart date", "Set Depart date to 2027-03-14", "depart", "2027-03-14"),
      blockedFill("travel_search", "Origin", "Fill Origin with the departure city"),
      select("travel_search", "Passengers", "Set Passengers to 3", "passengers", "3"),
      select("travel_search", "Cabin", "Choose Cabin Premium", "cabin", "Premium"),
      select("travel_search", "Stops", "Select Stops Nonstop", "stops", "Nonstop"),
      blockedSelect("travel_search", "Cabin", "Pick the business Cabin for the trip"),
      scrollDown("travel_search", "Scroll down to the fare calendar"),
      scrollUp("travel_search", "Scroll up to the search form"),
      historyBack("travel_search", "Go back to the travel page from before"),
      wait("travel_search", "Wait for the fare calendar to finish loading"),
    ],
  }),
);

// 7. Finance dashboard.
pages.push(
  page({
    id: "finance_dashboard",
    title: "Treasury overview",
    build: () => [
      elements.nav([
        { name: "View all transactions", href: "/transactions" },
        { name: "Download statement", href: "/statement" },
        { name: "Support", href: "/support" },
      ]),
      elements.heading(1, "Treasury overview"),
      elements.button("Overview", { role: "tab" }),
      elements.button("Statements", { role: "tab" }),
      elements.select("Date range", ["Last 7 days", "Last 30 days", "This quarter"]),
      elements.select("Account", ["Checking", "Savings", "Reserve"]),
      elements.select("Transaction type", ["All", "Debit", "Credit"]),
      elements.input("Search transactions", { type: "search" }),
      elements.input("Memo"),
      elements.input("Payee"),
      elements.button("Refresh data"),
      elements.button("Export CSV"),
      elements.button("Add transaction"),
      elements.button("Previous"),
      elements.button("Next"),
      elements.link("View budget", "/budget"),
      elements.spacer(2800),
    ],
    cases: [
      click("finance_dashboard", "Refresh data", "Refresh data on the treasury dashboard"),
      click("finance_dashboard", "Export CSV", "Export CSV for the selected range"),
      click("finance_dashboard", "Add transaction", "Add transaction to the ledger"),
      click("finance_dashboard", "Download statement", "Download statement for the account"),
      click("finance_dashboard", "Previous", "Go to Previous on the transaction table"),
      click("finance_dashboard", "Next", "Click Next for more transactions"),
      click("finance_dashboard", "Overview", "Open the Overview tab"),
      fill("finance_dashboard", "Search transactions", "Filter with Search transactions for payroll", "query", "payroll"),
      fill("finance_dashboard", "Search transactions", "Find vendors in Search transactions", "query", "vendor", "vendors"),
      fill("finance_dashboard", "Memo", "Add rent March to Memo", "memo", "rent March"),
      fill("finance_dashboard", "Memo", "Put utilities April in Memo", "memo", "utilities April", "utilities"),
      blockedFill("finance_dashboard", "Payee", "Fill Payee with the vendor name"),
      select("finance_dashboard", "Date range", "Set Date range to This quarter", "range", "This quarter"),
      select("finance_dashboard", "Account", "Choose Account Reserve", "account", "Reserve"),
      select("finance_dashboard", "Transaction type", "Set Transaction type to Credit", "type", "Credit"),
      blockedSelect("finance_dashboard", "Account", "Switch to the savings Account"),
      scrollDown("finance_dashboard", "Scroll down to the transaction table"),
      scrollUp("finance_dashboard", "Scroll up to the account summary"),
      historyBack("finance_dashboard", "Take me to the dashboard page before this one"),
      wait("finance_dashboard", "Wait for the balance widgets to settle"),
    ],
  }),
);

// 8. Media player.
pages.push(
  page({
    id: "media_player",
    title: "Field recordings",
    build: () => [
      elements.nav([
        { name: "Home", href: "/" },
        { name: "Subscriptions", href: "/subscriptions" },
      ]),
      elements.heading(1, "Field recordings"),
      elements.input("Search videos", { type: "search" }),
      elements.input("Search playlists", { type: "search" }),
      elements.textarea("Comment"),
      elements.select("Quality", ["Auto", "720p", "1080p"]),
      elements.select("Audio track", ["English", "Spanish"]),
      elements.select("Playback speed", ["0.5x", "1x", "1.5x"]),
      elements.button("Play"),
      elements.button("Pause"),
      elements.button("Mute"),
      elements.button("Fullscreen"),
      elements.button("Like"),
      elements.checkbox("Autoplay next video"),
      elements.link("Save to playlist", "/playlist"),
      elements.link("Share", "/share"),
      elements.link("Report", "/report"),
      elements.spacer(2600),
    ],
    cases: [
      click("media_player", "Play", "Play the current recording"),
      click("media_player", "Pause", "Pause the playback for a moment"),
      click("media_player", "Mute", "Mute the audio output"),
      click("media_player", "Fullscreen", "Go Fullscreen with the player"),
      click("media_player", "Like", "Like this recording"),
      click("media_player", "Save to playlist", "Save to playlist for later"),
      click("media_player", "Share", "Share the recording with a teammate"),
      fill("media_player", "Search videos", "Enter coastal storms in Search videos", "query", "coastal storms"),
      fill("media_player", "Search videos", "Look for tide pools in Search videos", "query", "tide pools", "tides"),
      fill("media_player", "Search playlists", "Put field notes into Search playlists", "query", "field notes"),
      fill("media_player", "Search playlists", "Find harbor sounds in Search playlists", "query", "harbor sounds", "harbor"),
      blockedFill("media_player", "Comment", "Fill Comment with a response"),
      select("media_player", "Quality", "Set Quality to 1080p", "quality", "1080p"),
      select("media_player", "Audio track", "Choose Audio track Spanish", "track", "Spanish"),
      select("media_player", "Playback speed", "Set Playback speed to 1.5x", "speed", "1.5x"),
      blockedSelect("media_player", "Quality", "Select the lowest Quality option"),
      scrollDown("media_player", "Scroll down to the recommended videos"),
      scrollUp("media_player", "Scroll up to the video title"),
      historyBack("media_player", "Return to the video list I came from"),
      wait("media_player", "Wait for the video player to become ready"),
    ],
  }),
);

// 9. Data table and pagination.
pages.push(
  page({
    id: "data_table",
    title: "Inventory table",
    build: () => [
      elements.link("Back to reports", "/reports"),
      elements.heading(1, "Inventory table"),
      elements.input("Filter rows", { type: "search" }),
      elements.input("Rename view"),
      elements.input("Search columns", { type: "search" }),
      elements.select("Rows per page", ["10", "25", "50"]),
      elements.select("Status filter", ["All", "Open", "Closed"]),
      elements.select("Owner", ["Anyone", "Me", "Team"]),
      elements.checkbox("Select all"),
      elements.checkbox("Select row 2"),
      elements.checkbox("Select row 5"),
      elements.button("Archive selected"),
      elements.button("Delete selected", { disabled: true }),
      elements.button("Export table"),
      elements.button("Open filters"),
      elements.button("Close filters"),
      elements.button("Previous page"),
      elements.button("Next page"),
      elements.button("First page"),
      elements.button("Last page"),
      elements.spacer(3000),
    ],
    cases: [
      click("data_table", "Select all", "Use Select all for the visible inventory rows"),
      click("data_table", "Select row 2", "Tick Select row 2 in the table"),
      click("data_table", "Archive selected", "Archive selected rows in the inventory"),
      click("data_table", "Export table", "Export table to a spreadsheet"),
      click("data_table", "Open filters", "Open filters above the table"),
      click("data_table", "Previous page", "Go to Previous page in the table"),
      click("data_table", "Next page", "Use Next page to see more rows"),
      fill("data_table", "Filter rows", "Enter blue jackets in Filter rows", "query", "blue jackets"),
      fill("data_table", "Filter rows", "Find SKU 8842 in Filter rows", "query", "SKU 8842", "sku"),
      fill("data_table", "Rename view", "Rename view to Winter stock", "name", "Winter stock"),
      fill("data_table", "Search columns", "Put quantity into Search columns", "query", "quantity"),
      blockedFill("data_table", "Filter rows", "Fill Filter rows with the SKU"),
      select("data_table", "Rows per page", "Set Rows per page to 50", "rows", "50"),
      select("data_table", "Status filter", "Choose Status filter Closed", "status", "Closed"),
      select("data_table", "Owner", "Set Owner to Team", "owner", "Team"),
      blockedSelect("data_table", "Rows per page", "Pick a smaller Rows per page value"),
      scrollDown("data_table", "Scroll down to the inventory footer"),
      scrollUp("data_table", "Scroll up to the table toolbar"),
      historyBack("data_table", "Go to the page before this table in history"),
      wait("data_table", "Wait for the rows to finish loading"),
    ],
  }),
);

// 10. Modal dialog.
pages.push(
  page({
    id: "modal_dialog",
    title: "Project workspace",
    build: () => [
      elements.nav([
        { name: "Projects", href: "/projects" },
        { name: "Manage members", href: "/members" },
      ]),
      elements.button("New project"),
      elements.paragraph("The invite dialog is open over the project list."),
      elements.raw('<div role="dialog" aria-label="Invite teammate">'),
      elements.heading(2, "Invite teammate"),
      elements.input("Work email", { type: "email" }),
      elements.textarea("Invite message"),
      elements.input("Search people", { type: "search" }),
      elements.select("Permission", ["Can view", "Can edit", "Admin"]),
      elements.select("Expires in", ["24 hours", "7 days", "30 days"]),
      elements.select("Notify via", ["Email", "Slack"]),
      elements.button("Send invite"),
      elements.button("Cancel"),
      elements.button("Copy invite link"),
      elements.button("Close dialog"),
      elements.link("Learn about permissions", "/permissions"),
      elements.link("Open help", "/help"),
      elements.link("Upgrade plan", "/upgrade"),
      elements.link("View billing", "/billing"),
      elements.raw("</div>"),
      elements.button("Dismiss banner"),
      elements.spacer(2600),
    ],
    cases: [
      click("modal_dialog", "Projects", "Open Projects behind the dialog"),
      click("modal_dialog", "New project", "Start a New project"),
      click("modal_dialog", "Send invite", "Send invite to the person in the dialog"),
      click("modal_dialog", "Cancel", "Cancel the invite without sending it"),
      click("modal_dialog", "Copy invite link", "Copy invite link for the teammate"),
      click("modal_dialog", "Close dialog", "Close dialog and return to the project list"),
      click("modal_dialog", "Dismiss banner", "Dismiss banner at the bottom"),
      fill("modal_dialog", "Work email", "Put the teammate address in Work email", "email", "teammate@example.com"),
      fill("modal_dialog", "Work email", "Invite the designer through Work email", "email", "designer@example.com", "designer"),
      fill("modal_dialog", "Invite message", "Add a welcome note in Invite message", "message", "Join the harbor project"),
      fill("modal_dialog", "Invite message", "Mention the kickoff in Invite message", "message", "Kickoff is Monday", "kickoff"),
      fill("modal_dialog", "Search people", "Look for Sam in Search people", "query", "Sam"),
      blockedFill("modal_dialog", "Work email", "Fill Work email with the invitee address"),
      select("modal_dialog", "Permission", "Set Permission to Can edit", "permission", "Can edit"),
      select("modal_dialog", "Expires in", "Choose Expires in 7 days", "expires", "7 days"),
      select("modal_dialog", "Notify via", "Notify via Slack", "notify", "Slack"),
      blockedSelect("modal_dialog", "Permission", "Pick a limited Permission for the guest"),
      scrollDown("modal_dialog", "Scroll down inside the dialog to the footer"),
      scrollUp("modal_dialog", "Scroll up to the dialog heading"),
      historyBack("modal_dialog", "Take me back to the page under this dialog"),
      wait("modal_dialog", "Wait for the permission list to load"),
    ],
  }),
);

// 11. Long article.
pages.push(
  page({
    id: "long_article",
    title: "Reading the coast",
    build: () => [
      elements.nav([
        { name: "Home", href: "/" },
        { name: "Article list", href: "/articles" },
      ]),
      elements.article([
        elements.heading(1, "Reading the coast"),
        elements.paragraph("An introduction to tide lines and long walks."),
        elements.spacer(2200),
        elements.link("Read the reviews", "/reviews"),
        elements.spacer(2200),
        elements.link("Back to top", "#top"),
        elements.link("Next article", "/next"),
        elements.link("Previous article", "/previous"),
        elements.link("Subscribe", "/subscribe"),
        elements.link("Share article", "/share"),
        elements.link("Report a typo", "/typo"),
      ]),
      elements.input("Add a comment"),
      elements.input("Name"),
      elements.input("Email", { type: "email" }),
      elements.select("Text size", ["Small", "Medium", "Large"]),
      elements.select("Theme", ["Light", "Dark", "Sepia"]),
      elements.select("Notifications", ["All", "Mentions", "None"]),
      elements.button("Post comment"),
      elements.spacer(2600),
    ],
    cases: [
      click("long_article", "Read the reviews", "Open Read the reviews for this essay"),
      click("long_article", "Back to top", "Use Back to top above the comments"),
      click("long_article", "Next article", "Open the Next article in the issue"),
      click("long_article", "Previous article", "Open the Previous article in the issue"),
      click("long_article", "Subscribe", "Subscribe to the coast series"),
      click("long_article", "Post comment", "Post comment under the essay"),
      fill("long_article", "Add a comment", "Write a response in Add a comment", "comment", "Lovely piece"),
      fill("long_article", "Add a comment", "Put a question in Add a comment", "comment", "Where is the trailhead?", "question"),
      fill("long_article", "Name", "Put Morgan in Name", "name", "Morgan"),
      fill("long_article", "Email", "Enter the address in Email", "email", "morgan@example.com"),
      fill("long_article", "Email", "Enter the backup address in Email", "email", "reader@example.com", "backup"),
      blockedFill("long_article", "Add a comment", "Fill Add a comment with my reaction"),
      select("long_article", "Text size", "Set Text size to Large", "size", "Large"),
      select("long_article", "Theme", "Choose Theme Sepia", "theme", "Sepia"),
      select("long_article", "Notifications", "Set Notifications to Mentions", "notifications", "Mentions"),
      blockedSelect("long_article", "Text size", "Pick a smaller Text size"),
      scrollDown("long_article", "Scroll down to the comment section"),
      scrollUp("long_article", "Scroll up toward the article introduction"),
      historyBack("long_article", "Go back to the article list I came from"),
      wait("long_article", "Wait for the reader comments to load"),
    ],
  }),
);

// 12. Support ticket.
pages.push(
  page({
    id: "support_ticket",
    title: "Ticket 4821",
    build: () => [
      elements.nav([
        { name: "Back to tickets", href: "/tickets" },
        { name: "View customer profile", href: "/customers/9" },
      ]),
      elements.heading(1, "Ticket 4821"),
      elements.input("Subject"),
      elements.textarea("Description"),
      elements.input("Customer email", { type: "email" }),
      elements.input("Search tickets", { type: "search" }),
      elements.select("Priority", ["Low", "Normal", "High", "Urgent"]),
      elements.select("Status", ["Open", "Pending", "Resolved"]),
      elements.select("Assignee", ["Unassigned", "Me", "Team lead"]),
      elements.checkbox("Notify customer"),
      elements.button("Save"),
      elements.button("Send reply"),
      elements.button("Close ticket"),
      elements.button("Attach file"),
      elements.button("Assign to me"),
      elements.button("Add internal note"),
      elements.link("Merge ticket", "/merge"),
      elements.spacer(2600),
    ],
    cases: [
      click("support_ticket", "Save", "Save the ticket changes"),
      click("support_ticket", "Send reply", "Send reply to the customer"),
      click("support_ticket", "Close ticket", "Close ticket as resolved"),
      click("support_ticket", "Attach file", "Attach file to the reply"),
      click("support_ticket", "Assign to me", "Assign to me for follow up"),
      click("support_ticket", "Notify customer", "Notify customer about the update"),
      click("support_ticket", "Back to tickets", "Return to Back to tickets list"),
      fill("support_ticket", "Subject", "Set Subject to Refund request", "subject", "Refund request"),
      fill("support_ticket", "Description", "Put the details in Description", "description", "The parcel arrived damaged."),
      fill("support_ticket", "Description", "Describe the late delivery in Description", "description", "The order is three days late.", "delay"),
      fill("support_ticket", "Customer email", "Enter the sender in Customer email", "email", "casey@example.com"),
      fill("support_ticket", "Customer email", "Use the billing address in Customer email", "email", "billing@example.com", "billing"),
      blockedFill("support_ticket", "Subject", "Fill Subject with a short summary"),
      select("support_ticket", "Priority", "Set Priority to Urgent", "priority", "Urgent"),
      select("support_ticket", "Status", "Choose Status Pending", "status", "Pending"),
      select("support_ticket", "Assignee", "Set Assignee to Team lead", "assignee", "Team lead"),
      blockedSelect("support_ticket", "Priority", "Pick a lower Priority for the ticket"),
      scrollDown("support_ticket", "Scroll down to the reply editor"),
      scrollUp("support_ticket", "Scroll up to the ticket header"),
      historyBack("support_ticket", "Return to the ticket queue I came from"),
      wait("support_ticket", "Wait for the ticket history to settle"),
      click("support_ticket", "Merge ticket", "Merge ticket into another case"),
    ],
  }),
);

// 13. Profile edit with similar save actions.
pages.push(
  page({
    id: "profile_edit",
    title: "Edit profile",
    build: () => [
      elements.nav([
        { name: "View public profile", href: "/u/me" },
        { name: "Change password", href: "/password" },
      ]),
      elements.heading(1, "Edit profile"),
      elements.input("Display name"),
      elements.input("Contact email", { type: "email" }),
      elements.input("Username", { disabled: true }),
      elements.textarea("Bio"),
      elements.select("Time zone", ["UTC", "Europe/Lisbon", "America/Chicago"]),
      elements.select("Language", ["English", "Portuguese", "German"]),
      elements.select("Profile visibility", ["Public", "Connections", "Private"]),
      elements.checkbox("Show email on profile"),
      elements.button("Save draft"),
      elements.button("Save and publish"),
      elements.button("Cancel"),
      elements.link("Upload photo", "/photo/upload"),
      elements.link("Remove photo", "/photo/remove"),
      elements.link("Privacy settings", "/privacy"),
      elements.spacer(2400),
    ],
    cases: [
      click("profile_edit", "Save draft", "Save draft without publishing the profile"),
      click("profile_edit", "Save and publish", "Save and publish the profile now"),
      click("profile_edit", "Cancel", "Cancel the profile changes"),
      click("profile_edit", "Upload photo", "Upload photo for the avatar"),
      click("profile_edit", "Remove photo", "Remove photo from the profile"),
      click("profile_edit", "Privacy settings", "Open Privacy settings for the profile"),
      click("profile_edit", "Show email on profile", "Turn on Show email on profile"),
      fill("profile_edit", "Display name", "Set Display name to Rowan Vale", "name", "Rowan Vale"),
      fill("profile_edit", "Display name", "Use R. Vale as the Display name", "name", "R. Vale", "short"),
      fill("profile_edit", "Contact email", "Put the new address in Contact email", "email", "rowan@example.com"),
      fill("profile_edit", "Bio", "Write the summary in Bio", "bio", "Coastal photographer."),
      fill("profile_edit", "Bio", "Add the studio history to Bio", "bio", "Ten years of field work.", "studio"),
      blockedFill("profile_edit", "Contact email", "Fill Contact email with the new address"),
      select("profile_edit", "Time zone", "Set Time zone to Europe/Lisbon", "time_zone", "Europe/Lisbon"),
      select("profile_edit", "Language", "Choose Language Portuguese", "language", "Portuguese"),
      select("profile_edit", "Profile visibility", "Set Profile visibility to Private", "visibility", "Private"),
      blockedSelect("profile_edit", "Language", "Select a different Language for the profile"),
      scrollDown("profile_edit", "Scroll down to the privacy settings"),
      scrollUp("profile_edit", "Scroll up to the profile header"),
      historyBack("profile_edit", "Go back to the profile page I came from"),
      wait("profile_edit", "Wait for the avatar preview to finish loading"),
    ],
  }),
);

// 14. Analytics report.
pages.push(
  page({
    id: "analytics_report",
    title: "Growth analytics",
    build: () => [
      elements.nav([
        { name: "View funnel", href: "/funnel" },
        { name: "View retention", href: "/retention" },
        { name: "Support", href: "/support" },
      ]),
      elements.heading(1, "Growth analytics"),
      elements.button("Traffic", { role: "tab" }),
      elements.button("Revenue", { role: "tab" }),
      elements.input("Report name"),
      elements.input("Filter users", { type: "search" }),
      elements.input("Search metrics", { type: "search" }),
      elements.select("Segment", ["All users", "Returning", "New"]),
      elements.select("Date range", ["Last 7 days", "Last 30 days", "Last 90 days"]),
      elements.select("Metric", ["Sessions", "Revenue", "Signups"]),
      elements.select("Comparison", ["None", "Previous period", "Same period last year"]),
      elements.checkbox("Include comparison"),
      elements.button("Refresh"),
      elements.button("Export"),
      elements.button("Schedule report"),
      elements.button("Share dashboard"),
      elements.button("Next page"),
      elements.button("Previous page"),
      elements.spacer(2800),
    ],
    cases: [
      click("analytics_report", "Refresh", "Refresh the analytics charts"),
      click("analytics_report", "Export", "Export the current analytics view"),
      click("analytics_report", "Schedule report", "Schedule report for Monday morning"),
      click("analytics_report", "Share dashboard", "Share dashboard with the growth team"),
      click("analytics_report", "View funnel", "Open View funnel for checkout steps"),
      click("analytics_report", "Revenue", "Switch to the Revenue tab"),
      click("analytics_report", "Include comparison", "Turn on Include comparison"),
      fill("analytics_report", "Report name", "Name the Report name Weekly growth", "name", "Weekly growth"),
      fill("analytics_report", "Report name", "Call the Report name Q4 board pack", "name", "Q4 board pack", "board"),
      fill("analytics_report", "Filter users", "Put enterprise in Filter users", "query", "enterprise"),
      fill("analytics_report", "Filter users", "Limit to trial users in Filter users", "query", "trial", "trial"),
      blockedFill("analytics_report", "Report name", "Fill Report name before saving"),
      select("analytics_report", "Segment", "Set Segment to Returning", "segment", "Returning"),
      select("analytics_report", "Date range", "Choose Date range Last 90 days", "range", "Last 90 days"),
      select("analytics_report", "Metric", "Set Metric to Revenue", "metric", "Revenue"),
      blockedSelect("analytics_report", "Metric", "Pick the Signups Metric"),
      scrollDown("analytics_report", "Scroll down to the cohort table"),
      scrollUp("analytics_report", "Scroll up to the KPI cards"),
      historyBack("analytics_report", "Go back to the dashboard I opened before"),
      wait("analytics_report", "Wait for the charts to render"),
    ],
  }),
);

// 15. Billing.
pages.push(
  page({
    id: "billing_settings",
    title: "Billing and plans",
    build: () => [
      elements.nav([
        { name: "View pricing", href: "/pricing" },
        { name: "Contact sales", href: "/sales" },
        { name: "Back to account", href: "/account" },
      ]),
      elements.heading(1, "Billing and plans"),
      elements.input("Card number"),
      elements.input("Name on card"),
      elements.input("Billing ZIP"),
      elements.input("Promo code"),
      elements.select("Plan", ["Starter", "Growth", "Enterprise"]),
      elements.select("Billing cycle", ["Monthly", "Annual"]),
      elements.select("Currency", ["USD", "EUR", "GBP"]),
      elements.checkbox("Auto-renew"),
      elements.button("Update payment method"),
      elements.button("Cancel subscription"),
      elements.button("Download invoices"),
      elements.button("Apply promo code"),
      elements.button("Manage seats"),
      elements.spacer(2400),
    ],
    cases: [
      click("billing_settings", "Back to account", "Return to Back to account settings"),
      click("billing_settings", "Update payment method", "Update payment method with the new card"),
      click("billing_settings", "Cancel subscription", "Cancel subscription at the end of term"),
      click("billing_settings", "Download invoices", "Download invoices for the tax year"),
      click("billing_settings", "Apply promo code", "Apply promo code to the next invoice"),
      click("billing_settings", "Auto-renew", "Turn off Auto-renew for the plan"),
      fill("billing_settings", "Card number", "Enter the card in Card number", "card", "4242 4242 4242 4242"),
      fill("billing_settings", "Name on card", "Put the holder in Name on card", "name", "ROWAN VALE"),
      fill("billing_settings", "Name on card", "Use the company name in Name on card", "name", "HARBOR LABS LLC", "company"),
      fill("billing_settings", "Billing ZIP", "Set Billing ZIP to 97201", "zip", "97201"),
      fill("billing_settings", "Promo code", "Enter LAUNCH25 in Promo code", "promo", "LAUNCH25"),
      blockedFill("billing_settings", "Billing ZIP", "Fill Billing ZIP for the receipt"),
      select("billing_settings", "Plan", "Set Plan to Growth", "plan", "Growth"),
      select("billing_settings", "Billing cycle", "Choose Billing cycle Annual", "cycle", "Annual"),
      select("billing_settings", "Currency", "Set Currency to EUR", "currency", "EUR"),
      blockedSelect("billing_settings", "Plan", "Pick the Enterprise Plan"),
      scrollDown("billing_settings", "Scroll down to the invoice history"),
      scrollUp("billing_settings", "Scroll up to the payment form"),
      historyBack("billing_settings", "Take me back to the account page from before"),
      wait("billing_settings", "Wait for the invoice list to load"),
    ],
  }),
);

// 16. Onboarding wizard.
pages.push(
  page({
    id: "onboarding_wizard",
    title: "Set up your workspace",
    build: () => [
      elements.nav([
        { name: "Account", href: "/onboarding/account" },
        { name: "Team", href: "/onboarding/team" },
        { name: "Log out", href: "/logout" },
      ]),
      elements.heading(1, "Set up your workspace"),
      elements.input("Company name"),
      elements.input("Workspace URL"),
      elements.input("Search integrations", { type: "search" }),
      elements.select("Team size", ["1-10", "11-50", "51-200"]),
      elements.select("Industry", ["Software", "Retail", "Healthcare"]),
      elements.select("Region", ["US", "EU", "APAC"]),
      elements.checkbox("Invite teammates later"),
      elements.button("Next step"),
      elements.button("Skip for now"),
      elements.button("Save progress"),
      elements.button("Open setup guide"),
      elements.button("Restart tour"),
      elements.link("Back", "/onboarding/back"),
      elements.link("Need help?", "/help"),
      elements.link("Contact support", "/support"),
      elements.spacer(2600),
    ],
    cases: [
      click("onboarding_wizard", "Next step", "Continue with Next step in the wizard"),
      click("onboarding_wizard", "Skip for now", "Skip for now and leave this step for later"),
      click("onboarding_wizard", "Save progress", "Save progress before leaving the wizard"),
      click("onboarding_wizard", "Open setup guide", "Open setup guide in a new tab"),
      click("onboarding_wizard", "Restart tour", "Restart tour from the first screen"),
      click("onboarding_wizard", "Invite teammates later", "Choose Invite teammates later"),
      click("onboarding_wizard", "Contact support", "Contact support about onboarding"),
      fill("onboarding_wizard", "Company name", "Set Company name to Harbor Labs", "company", "Harbor Labs"),
      fill("onboarding_wizard", "Company name", "Use Northwind Studio for Company name", "company", "Northwind Studio", "alternate"),
      fill("onboarding_wizard", "Workspace URL", "Use harbor-labs in Workspace URL", "url", "harbor-labs"),
      fill("onboarding_wizard", "Search integrations", "Look for Slack in Search integrations", "query", "Slack"),
      fill("onboarding_wizard", "Search integrations", "Find GitHub in Search integrations", "query", "GitHub", "github"),
      blockedFill("onboarding_wizard", "Company name", "Fill Company name for the workspace"),
      select("onboarding_wizard", "Team size", "Set Team size to 11-50", "size", "11-50"),
      select("onboarding_wizard", "Industry", "Choose Industry Healthcare", "industry", "Healthcare"),
      select("onboarding_wizard", "Region", "Set Region to EU", "region", "EU"),
      blockedSelect("onboarding_wizard", "Industry", "Select the Software Industry"),
      scrollDown("onboarding_wizard", "Scroll down to the setup checklist"),
      scrollUp("onboarding_wizard", "Scroll up to the workspace name field"),
      historyBack("onboarding_wizard", "Go back to the onboarding page before this one"),
      wait("onboarding_wizard", "Wait for the workspace check to complete"),
    ],
  }),
);

function collectNames(element) {
  const names = element.names ? element.names.slice() : element.name ? [element.name] : [];
  for (const child of element.children ?? []) names.push(...collectNames(child));
  return names;
}

// Build and verify the package before writing anything.
const cases = pages.flatMap((spec) => spec.cases);
const names = new Set();
for (const testCase of cases) {
  if (names.has(testCase.name)) throw new Error(`duplicate case name: ${testCase.name}`);
  names.add(testCase.name);
  const spec = pages.find((candidate) => candidate.id === testCase.fixture.replace(/\.html$/, ""));
  if (!spec) throw new Error(`unknown fixture for ${testCase.name}`);
  if (testCase.expect.target_name) {
    const matches = spec.elements.flatMap(collectNames);
    const found = matches.filter((name) => name === testCase.expect.target_name).length;
    if (found !== 1) {
      throw new Error(
        `${testCase.name}: expected target ${JSON.stringify(
          testCase.expect.target_name,
        )} must appear exactly once, found ${found}`,
      );
    }
    if (testCase.expect.blocked && testCase.values) {
      throw new Error(`${testCase.name}: blocked case must not supply values`);
    }
  }
}

const byOperation = {};
for (const testCase of cases) {
  const operation = testCase.expect.operation;
  byOperation[operation] = (byOperation[operation] ?? 0) + 1;
}
const blockedCases = cases.filter((testCase) => testCase.expect.blocked).length;
if (cases.length < 320) {
  throw new Error(`expected at least 320 cases, built ${cases.length}`);
}

mkdirSync(FIXTURES, { recursive: true });
for (const spec of pages) {
  writeFileSync(join(FIXTURES, `${spec.id}.html`), renderPage(spec));
}
writeFileSync(join(HERE, "cases.json"), `${JSON.stringify({ cases }, null, 2)}\n`);

const manifest = {
  seed: SEED,
  generated_by: "cli/evals/decide/v2/generate.mjs",
  generated_on: "2026-11-08",
  label_source:
    "Case labels were written from each page's element list and instruction intent before any decider call. No label is derived from model output.",
  pages: pages.length,
  cases: cases.length,
  blocked_cases: blockedCases,
  by_operation: byOperation,
  notes: [
    "Fixtures are rendered from the element lists in generate.mjs.",
    "Target accessible names appear exactly once per page; duplicate distractor names are intentional and never labeled.",
    "Blocked cases are fill or select requests whose caller supplied no value.",
    "The seed is fixed at 20261108 for reproducibility; fixture text and labels contain no randomness.",
  ],
};
writeFileSync(join(HERE, "manifest.json"), `${JSON.stringify(manifest, null, 2)}\n`);

console.log(
  `wrote ${pages.length} fixtures and ${cases.length} cases (${blockedCases} blocked) to ${HERE}`,
);
console.log(JSON.stringify(byOperation));
