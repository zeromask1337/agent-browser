#!/usr/bin/env node
// Deterministic generator for the v3 decision eval set.
//
// Every fixture is rendered from the element list below, in declaration order.
// Every case names the operation, the expected accessible name, and the values
// the caller supplies. The labels were written from those element lists and
// fixture construction before any decider call, and are never derived from
// model output.
//
// Run from anywhere:
//   node cli/evals/decide/v3/generate.mjs
//
// The seed is fixed and recorded for reproducibility. Fixture text and labels
// contain no randomness, so output is byte stable.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SEED = 20261109;
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
  const id = `q${nextId}`;
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
      name: "nav",
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
      (id, extra) => `<input id="${id}" type="${options.type || "text"}"${extra}>`,
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
    const id = `q${nextId}`;
    return {
      name,
      render(indent) {
        const disabled = options.disabled ? " disabled" : "";
        return `${indent}<input id="${id}" type="checkbox"${disabled}>\n${indent}<label for="${id}">${name}</label>`;
      },
    };
  },
  heading(level, text) {
    return {
      name: "heading",
      render(indent) {
        return `${indent}<h${level}>${text}</h${level}>`;
      },
    };
  },
  paragraph(text) {
    return {
      name: "paragraph",
      render(indent) {
        return `${indent}<p>${text}</p>`;
      },
    };
  },
  spacer(height) {
    return {
      name: "spacer",
      render(indent) {
        return `${indent}<div style="height: ${height}px"> </div>`;
      },
    };
  },
  section(title, children) {
    return {
      name: "section",
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
      name: "form",
      render(indent) {
        return `${indent}<form>\n${html(children, `${indent}  `)}\n${indent}</form>`;
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

function clickSuffix(pageId, target, instruction, suffix) {
  return targetCase(pageId, "click", target, instruction, undefined, suffix);
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

// 1. Library catalog.
pages.push(
  page({
    id: "library_catalog",
    title: "Riverside library catalog",
    build: () => [
      elements.nav([
        { name: "Branches", href: "/branches" },
        { name: "My loans", href: "/loans" },
        { name: "Reading lists", href: "/lists" },
      ]),
      elements.heading(1, "Riverside library"),
      elements.input("Catalog search", { type: "search" }),
      elements.button("Search catalog"),
      elements.select("Genre", ["Any genre", "Mystery", "History", "Poetry"]),
      elements.select("Format", ["Any format", "Large print", "Audiobook", "Ebook"]),
      elements.select("Sort order", ["Relevance", "Newest", "Title"]),
      elements.checkbox("Available now"),
      elements.checkbox("Include branch-only copies", { disabled: true }),
      elements.button("Reserve this item"),
      elements.link("Read online", "/read"),
      elements.link("Place a hold", "/hold"),
      elements.button("Previous page"),
      elements.button("Next page"),
      elements.spacer(1900),
      elements.link("Branch hours", "/hours"),
    ],
    cases: [
      click("library_catalog", "Search catalog", "Start the catalog search for the typed title"),
      click("library_catalog", "Reserve this item", "Reserve this item for pickup at Riverside"),
      click("library_catalog", "Read online", "Open Read online for the selected book"),
      click("library_catalog", "Place a hold", "Place a hold on the checked-out title"),
      click("library_catalog", "Available now", "Limit results to Available now"),
      clickSuffix(
        "library_catalog",
        "Previous page",
        "Go back to the previous page of results",
        "pagination",
      ),
      fill("library_catalog", "Catalog search", "Type island lighthouses in Catalog search", "query", "island lighthouses"),
      fill("library_catalog", "Catalog search", "Look up coastal maps in Catalog search", "query", "coastal maps", "maps"),
      select("library_catalog", "Genre", "Set Genre to Poetry", "genre", "Poetry"),
      select("library_catalog", "Format", "Choose Format Audiobook", "format", "Audiobook"),
      select("library_catalog", "Sort order", "Change Sort order to Newest", "sort", "Newest"),
      blockedFill("library_catalog", "Catalog search", "Enter the title I forgot in Catalog search"),
      blockedSelect("library_catalog", "Format", "Pick a different Format for the request"),
      scrollDown("library_catalog", "Scroll down to the branch hours link"),
      scrollUp("library_catalog", "Scroll up to the catalog search box"),
      historyBack("library_catalog", "Take me back to the search page I came from"),
      wait("library_catalog", "Wait for the availability counts to load"),
    ],
  }),
);

// 2. Clinic intake.
pages.push(
  page({
    id: "clinic_intake",
    title: "Cedar clinic intake",
    build: () => [
      elements.nav([
        { name: "Appointments", href: "/appointments" },
        { name: "Records", href: "/records" },
        { name: "Billing", href: "/billing" },
      ]),
      elements.heading(1, "Patient intake"),
      elements.form([
        elements.input("Legal name"),
        elements.input("Date of birth", { type: "text" }),
        elements.input("Mobile number", { type: "text" }),
        elements.select("Insurance plan", ["Self pay", "Cedar Basic", "Cedar Plus"]),
        elements.select("Visit reason", ["Checkup", "Follow-up", "Urgent"]),
        elements.select("Preferred location", ["Downtown", "Westside"]),
        elements.checkbox("New patient"),
        elements.textarea("Symptoms"),
        elements.button("Check in"),
        elements.button("Save draft"),
      ]),
      elements.link("Prepare for your visit", "/prepare"),
      elements.link("Patient rights", "/rights"),
      elements.spacer(2100),
      elements.button("Request interpreter"),
    ],
    cases: [
      click("clinic_intake", "Check in", "Check in the patient at the front desk"),
      click("clinic_intake", "Save draft", "Save draft of the intake form"),
      click("clinic_intake", "New patient", "Mark this person as a New patient"),
      click("clinic_intake", "Prepare for your visit", "Open Prepare for your visit instructions"),
      click("clinic_intake", "Patient rights", "Read the Patient rights notice"),
      click("clinic_intake", "Request interpreter", "Request interpreter help for the appointment"),
      fill("clinic_intake", "Legal name", "Put Dana Whitfield in Legal name", "name", "Dana Whitfield"),
      fill("clinic_intake", "Legal name", "Use Dana's married name in Legal name", "name", "Dana Okafor", "married"),
      fill("clinic_intake", "Mobile number", "Enter the callback number in Mobile number", "phone", "555-0140"),
      fill("clinic_intake", "Symptoms", "Describe the symptoms in Symptoms", "symptoms", "dizziness for two days"),
      fill("clinic_intake", "Date of birth", "Set Date of birth to 1988-04-12", "dob", "1988-04-12"),
      blockedFill("clinic_intake", "Legal name", "Fill Legal name from the paper chart"),
      select("clinic_intake", "Insurance plan", "Switch Insurance plan to Cedar Plus", "plan", "Cedar Plus"),
      select("clinic_intake", "Visit reason", "Set Visit reason to Follow-up", "reason", "Follow-up"),
      select("clinic_intake", "Preferred location", "Choose Preferred location Westside", "location", "Westside"),
      blockedSelect("clinic_intake", "Visit reason", "Pick the closest Visit reason option"),
      scrollDown("clinic_intake", "Scroll down to the interpreter request"),
      scrollUp("clinic_intake", "Scroll up to the patient name fields"),
      historyBack("clinic_intake", "Return to the appointment list I opened before this form"),
      wait("clinic_intake", "Wait for the insurance eligibility check to finish"),
    ],
  }),
);

// 3. Freight board.
pages.push(
  page({
    id: "freight_board",
    title: "Harbor freight board",
    build: () => [
      elements.nav([
        { name: "Loads", href: "/loads" },
        { name: "Carriers", href: "/carriers" },
        { name: "Invoices", href: "/invoices" },
      ]),
      elements.heading(1, "Open loads"),
      elements.input("Origin city", { type: "search" }),
      elements.button("Find loads"),
      elements.select("Trailer", ["Any trailer", "Dry van", "Reefer", "Flatbed"]),
      elements.select("Weight band", ["Any weight", "Under 10k", "10k to 20k"]),
      elements.select("Sort by", ["Pickup date", "Rate", "Distance"]),
      elements.checkbox("Team drivers only"),
      elements.button("Bid on load"),
      elements.link("Load details", "/load/42"),
      elements.button("Save search"),
      elements.button("Clear filters"),
      elements.button("Next page"),
      elements.spacer(1800),
      elements.link("Carrier requirements", "/requirements"),
    ],
    cases: [
      click("freight_board", "Find loads", "Run Find loads for the entered origin"),
      click("freight_board", "Bid on load", "Bid on load 42 at the posted rate"),
      click("freight_board", "Load details", "Open Load details before bidding"),
      click("freight_board", "Save search", "Save search for the Texas lane"),
      click("freight_board", "Clear filters", "Clear filters and show all trailers"),
      click("freight_board", "Team drivers only", "Restrict the board to Team drivers only"),
      fill("freight_board", "Origin city", "Put Dallas in Origin city", "origin", "Dallas"),
      fill("freight_board", "Origin city", "Use Laredo as the Origin city", "origin", "Laredo", "laredo"),
      select("freight_board", "Trailer", "Set Trailer to Reefer", "trailer", "Reefer"),
      select("freight_board", "Weight band", "Choose Weight band 10k to 20k", "weight", "10k to 20k"),
      select("freight_board", "Sort by", "Change Sort by to Rate", "sort", "Rate"),
      blockedFill("freight_board", "Origin city", "Fill Origin city with the shipper's town"),
      blockedSelect("freight_board", "Trailer", "Select the trailer type we discussed"),
      scrollDown("freight_board", "Scroll down to the carrier requirements link"),
      scrollUp("freight_board", "Scroll up to the load search controls"),
      historyBack("freight_board", "Go back to the carrier page from before this board"),
      wait("freight_board", "Wait until the rate estimates appear"),
    ],
  }),
);

// 4. Museum tickets.
pages.push(
  page({
    id: "museum_tickets",
    title: "Juniper museum tickets",
    build: () => [
      elements.nav([
        { name: "Visit", href: "/visit" },
        { name: "Exhibits", href: "/exhibits" },
        { name: "Membership", href: "/membership" },
      ]),
      elements.heading(1, "Plan your museum visit"),
      elements.input("Visit date", { type: "text" }),
      elements.select("Ticket type", ["Adult", "Child", "Senior", "Student"]),
      elements.select("Entry time", ["10:00", "12:00", "14:00"]),
      elements.select("Gallery add-on", ["None", "Special exhibition", "Sculpture garden"]),
      elements.input("Group size", { type: "text" }),
      elements.checkbox("Wheelchair access"),
      elements.checkbox("Audio guide"),
      elements.button("Add to basket"),
      elements.link("Exhibit calendar", "/calendar"),
      elements.link("Accessibility map", "/accessibility"),
      elements.spacer(2200),
      elements.button("Buy membership"),
    ],
    cases: [
      click("museum_tickets", "Add to basket", "Add the selected tickets to the basket"),
      click("museum_tickets", "Exhibit calendar", "Open the Exhibit calendar for next month"),
      click("museum_tickets", "Accessibility map", "View the Accessibility map before visiting"),
      click("museum_tickets", "Wheelchair access", "Request Wheelchair access for the group"),
      click("museum_tickets", "Audio guide", "Include an Audio guide with the tickets"),
      click("museum_tickets", "Buy membership", "Buy membership instead of day tickets"),
      fill("museum_tickets", "Visit date", "Enter the visit date as 2026-12-04", "date", "2026-12-04"),
      fill("museum_tickets", "Visit date", "Use the school holiday date in Visit date", "date", "2026-12-18", "holiday"),
      fill("museum_tickets", "Group size", "Set Group size to six", "size", "6"),
      select("museum_tickets", "Ticket type", "Choose Ticket type Student", "ticket", "Student"),
      select("museum_tickets", "Entry time", "Set Entry time to 14:00", "time", "14:00"),
      select("museum_tickets", "Gallery add-on", "Add the Special exhibition as the Gallery add-on", "addon", "Special exhibition"),
      blockedFill("museum_tickets", "Visit date", "Fill Visit date from the invitation"),
      blockedSelect("museum_tickets", "Entry time", "Pick the Entry time that suits us"),
      scrollDown("museum_tickets", "Scroll down to the membership button"),
      scrollUp("museum_tickets", "Scroll up to the ticket date field"),
      historyBack("museum_tickets", "Take me back to the exhibit page under this ticket form"),
      wait("museum_tickets", "Wait for the ticket prices to load"),
    ],
  }),
);

// 5. Recipe box.
pages.push(
  page({
    id: "recipe_box",
    title: "Copper pot recipes",
    build: () => [
      elements.nav([
        { name: "Browse", href: "/browse" },
        { name: "Saved", href: "/saved" },
        { name: "Shopping list", href: "/list" },
      ]),
      elements.heading(1, "Weeknight recipes"),
      elements.input("Search recipes", { type: "search" }),
      elements.button("Find recipes"),
      elements.select("Cuisine", ["Any cuisine", "Thai", "Italian", "Mexican"]),
      elements.select("Diet", ["No filter", "Vegetarian", "Gluten free"]),
      elements.select("Cook time", ["Any time", "Under 30 minutes", "Under an hour"]),
      elements.checkbox("Use pantry items only"),
      elements.button("Save recipe"),
      elements.button("Add to shopping list"),
      elements.link("Review this recipe", "/reviews"),
      elements.link("Print recipe", "/print"),
      elements.spacer(2000),
      elements.link("Cooking terms", "/terms"),
    ],
    cases: [
      click("recipe_box", "Find recipes", "Search recipes for the typed ingredient"),
      click("recipe_box", "Save recipe", "Save recipe to the weeknight collection"),
      click("recipe_box", "Add to shopping list", "Add to shopping list for Saturday"),
      click("recipe_box", "Review this recipe", "Read the Review this recipe notes"),
      click("recipe_box", "Print recipe", "Print recipe for the kitchen counter"),
      click("recipe_box", "Use pantry items only", "Filter to Use pantry items only"),
      fill("recipe_box", "Search recipes", "Enter chickpea stew in Search recipes", "query", "chickpea stew"),
      fill("recipe_box", "Search recipes", "Look up lime dessert in Search recipes", "query", "lime dessert", "lime"),
      select("recipe_box", "Cuisine", "Set Cuisine to Thai", "cuisine", "Thai"),
      select("recipe_box", "Diet", "Choose Diet Gluten free", "diet", "Gluten free"),
      select("recipe_box", "Cook time", "Change Cook time to Under 30 minutes", "time", "Under 30 minutes"),
      blockedFill("recipe_box", "Search recipes", "Fill Search recipes with the dish name"),
      blockedSelect("recipe_box", "Diet", "Pick the diet my guest needs"),
      scrollDown("recipe_box", "Scroll down to the cooking terms link"),
      scrollUp("recipe_box", "Scroll up to the recipe title"),
      historyBack("recipe_box", "Return to the saved list I came from"),
      wait("recipe_box", "Wait for the recipe cards to finish loading"),
    ],
  }),
);

// 6. Hotel booking.
pages.push(
  page({
    id: "hotel_booking",
    title: "Lantern hotel booking",
    build: () => [
      elements.nav([
        { name: "Rooms", href: "/rooms" },
        { name: "Offers", href: "/offers" },
        { name: "Reservations", href: "/reservations" },
      ]),
      elements.heading(1, "Book a room"),
      elements.input("Check-in", { type: "text" }),
      elements.input("Check-out", { type: "text" }),
      elements.select("Guests", ["1 adult", "2 adults", "2 adults and 1 child"]),
      elements.select("Room type", ["Standard", "Deluxe", "Suite"]),
      elements.select("Bed preference", ["King", "Two queens"]),
      elements.checkbox("Breakfast included"),
      elements.input("Promo code", { type: "text" }),
      elements.button("Check availability"),
      elements.link("Cancellation policy", "/policy"),
      elements.link("Nearby restaurants", "/nearby"),
      elements.spacer(2300),
      elements.button("Book now"),
    ],
    cases: [
      click("hotel_booking", "Check availability", "Check availability for the entered dates"),
      click("hotel_booking", "Book now", "Book now at the quoted rate"),
      click("hotel_booking", "Cancellation policy", "Read the Cancellation policy"),
      click("hotel_booking", "Nearby restaurants", "Open Nearby restaurants for dinner"),
      click("hotel_booking", "Breakfast included", "Add Breakfast included to the stay"),
      fill("hotel_booking", "Check-in", "Enter the Check-in date as 2026-11-20", "checkin", "2026-11-20"),
      fill("hotel_booking", "Check-in", "Use the earlier Check-in date", "checkin", "2026-11-18", "earlier"),
      fill("hotel_booking", "Check-out", "Set Check-out to 2026-11-24", "checkout", "2026-11-24"),
      fill("hotel_booking", "Promo code", "Apply LANTERN20 in Promo code", "promo", "LANTERN20"),
      select("hotel_booking", "Guests", "Set Guests to 2 adults and 1 child", "guests", "2 adults and 1 child"),
      select("hotel_booking", "Room type", "Choose Room type Suite", "room", "Suite"),
      select("hotel_booking", "Bed preference", "Switch Bed preference to Two queens", "bed", "Two queens"),
      blockedFill("hotel_booking", "Check-in", "Fill Check-in with the arrival date"),
      blockedSelect("hotel_booking", "Room type", "Pick the room type from the company rate"),
      scrollDown("hotel_booking", "Scroll down to the book now button"),
      scrollUp("hotel_booking", "Scroll up to the date fields"),
      historyBack("hotel_booking", "Go back to the room list from before this booking"),
      wait("hotel_booking", "Wait for the room rates to appear"),
    ],
  }),
);

// 7. Bank transfer.
pages.push(
  page({
    id: "bank_transfer",
    title: "Northline bank transfer",
    build: () => [
      elements.nav([
        { name: "Accounts", href: "/accounts" },
        { name: "Payees", href: "/payees" },
        { name: "Statements", href: "/statements" },
      ]),
      elements.heading(1, "Transfer money"),
      elements.form([
        elements.select("From account", ["Checking", "Savings", "Joint checking"]),
        elements.select("To payee", ["Ada Rentals", "City Utilities", "Nora Chen"]),
        elements.input("Amount", { type: "text" }),
        elements.input("Transfer date", { type: "text" }),
        elements.select("Frequency", ["One time", "Weekly", "Monthly"]),
        elements.textarea("Memo"),
        elements.checkbox("Send email receipt"),
        elements.button("Review transfer"),
      ]),
      elements.link("Transfer limits", "/limits"),
      elements.link("Report fraud", "/fraud"),
      elements.spacer(2100),
      elements.button("Confirm transfer", { disabled: true }),
    ],
    cases: [
      click("bank_transfer", "Review transfer", "Review transfer before sending it"),
      click("bank_transfer", "Transfer limits", "Check the Transfer limits for this account"),
      click("bank_transfer", "Report fraud", "Report fraud on the last transfer"),
      click("bank_transfer", "Send email receipt", "Enable Send email receipt"),
      fill("bank_transfer", "Amount", "Put 240.50 in Amount", "amount", "240.50"),
      fill("bank_transfer", "Amount", "Use 1200 as the Amount", "amount", "1200", "rent"),
      fill("bank_transfer", "Transfer date", "Set Transfer date to 2026-11-15", "date", "2026-11-15"),
      fill("bank_transfer", "Memo", "Write November rent in Memo", "memo", "November rent"),
      select("bank_transfer", "From account", "Move money from Joint checking", "from", "Joint checking"),
      select("bank_transfer", "To payee", "Send it to Nora Chen", "payee", "Nora Chen"),
      select("bank_transfer", "Frequency", "Make the transfer Monthly", "frequency", "Monthly"),
      blockedFill("bank_transfer", "Memo", "Fill Memo with the invoice number"),
      blockedSelect("bank_transfer", "Frequency", "Choose the repeat schedule we agreed"),
      scrollDown("bank_transfer", "Scroll down to the transfer limits link"),
      scrollUp("bank_transfer", "Scroll up to the amount field"),
      historyBack("bank_transfer", "Take me back to the account page from before this transfer"),
      wait("bank_transfer", "Wait for the payee balance to settle"),
    ],
  }),
);

// 8. CRM pipeline.
pages.push(
  page({
    id: "crm_pipeline",
    title: "Beacon CRM pipeline",
    build: () => [
      elements.nav([
        { name: "Leads", href: "/leads" },
        { name: "Accounts", href: "/accounts" },
        { name: "Reports", href: "/reports" },
      ]),
      elements.heading(1, "Sales pipeline"),
      elements.button("Qualified", { role: "tab" }),
      elements.button("Proposal", { role: "tab" }),
      elements.button("Closed", { role: "tab" }),
      elements.input("Search deals", { type: "search" }),
      elements.button("New deal"),
      elements.select("Owner", ["Anyone", "Priya Shah", "Marco Ruiz"]),
      elements.select("Stage", ["Qualified", "Proposal", "Negotiation", "Closed"]),
      elements.select("Close month", ["November", "December", "January"]),
      elements.checkbox("At risk only"),
      elements.link("Pipeline guide", "/guide"),
      elements.link("Import deals", "/import"),
      elements.spacer(2000),
      elements.button("Export report"),
    ],
    cases: [
      click("crm_pipeline", "New deal", "Create a New deal for the inbound lead"),
      click("crm_pipeline", "Proposal", "Open the Proposal tab of the pipeline"),
      click("crm_pipeline", "Closed", "Review the Closed tab"),
      click("crm_pipeline", "Pipeline guide", "Read the Pipeline guide for stage rules"),
      click("crm_pipeline", "Import deals", "Import deals from the spreadsheet"),
      click("crm_pipeline", "At risk only", "Show At risk only deals"),
      fill("crm_pipeline", "Search deals", "Find Acme renewal in Search deals", "query", "Acme renewal"),
      fill("crm_pipeline", "Search deals", "Look up Beacon Labs in Search deals", "query", "Beacon Labs", "labs"),
      select("crm_pipeline", "Owner", "Assign Owner Priya Shah", "owner", "Priya Shah"),
      select("crm_pipeline", "Stage", "Set Stage to Negotiation", "stage", "Negotiation"),
      select("crm_pipeline", "Close month", "Change Close month to December", "month", "December"),
      blockedFill("crm_pipeline", "Search deals", "Fill Search deals with the account name"),
      blockedSelect("crm_pipeline", "Owner", "Select the deal owner from the handoff"),
      scrollDown("crm_pipeline", "Scroll down to the export report button"),
      scrollUp("crm_pipeline", "Scroll up to the pipeline tabs"),
      historyBack("crm_pipeline", "Go back to the account page I opened before this pipeline"),
      wait("crm_pipeline", "Wait for the deal totals to load"),
    ],
  }),
);

// 9. School portal.
pages.push(
  page({
    id: "school_portal",
    title: "Maple school portal",
    build: () => [
      elements.nav([
        { name: "Classes", href: "/classes" },
        { name: "Grades", href: "/grades" },
        { name: "Calendar", href: "/calendar" },
      ]),
      elements.heading(1, "Family portal"),
      elements.input("Student name", { type: "text" }),
      elements.input("Guardian email", { type: "email" }),
      elements.select("Term", ["Fall", "Winter", "Spring"]),
      elements.select("Class", ["Biology", "Algebra", "World history"]),
      elements.select("Contact method", ["Email", "Phone", "Text message"]),
      elements.checkbox("Receive weekly digest"),
      elements.button("Update preferences"),
      elements.link("Attendance record", "/attendance"),
      elements.link("Teacher directory", "/teachers"),
      elements.spacer(2200),
      elements.button("Download transcript"),
    ],
    cases: [
      click("school_portal", "Update preferences", "Update preferences for the family account"),
      click("school_portal", "Attendance record", "Open the Attendance record"),
      click("school_portal", "Teacher directory", "Look up a teacher in the Teacher directory"),
      click("school_portal", "Receive weekly digest", "Turn on Receive weekly digest"),
      click("school_portal", "Download transcript", "Download transcript for the term"),
      fill("school_portal", "Student name", "Enter Mira Patel in Student name", "student", "Mira Patel"),
      fill("school_portal", "Student name", "Use Mira's legal name in Student name", "student", "Mira Desai", "legal"),
      fill("school_portal", "Guardian email", "Set Guardian email to parent@example.com", "email", "parent@example.com"),
      select("school_portal", "Term", "Set Term to Winter", "term", "Winter"),
      select("school_portal", "Class", "Choose Class World history", "class", "World history"),
      select("school_portal", "Contact method", "Switch Contact method to Text message", "contact", "Text message"),
      blockedFill("school_portal", "Student name", "Fill Student name from the enrollment form"),
      blockedSelect("school_portal", "Term", "Pick the term on the report card"),
      scrollDown("school_portal", "Scroll down to the transcript button"),
      scrollUp("school_portal", "Scroll up to the student fields"),
      historyBack("school_portal", "Return to the class list I came from"),
      wait("school_portal", "Wait for the grade summary to load"),
    ],
  }),
);

// 10. IoT console.
pages.push(
  page({
    id: "iot_console",
    title: "Grid IoT console",
    build: () => [
      elements.nav([
        { name: "Devices", href: "/devices" },
        { name: "Alerts", href: "/alerts" },
        { name: "Firmware", href: "/firmware" },
      ]),
      elements.heading(1, "Sensor fleet"),
      elements.input("Device search", { type: "search" }),
      elements.button("Search devices"),
      elements.select("Site", ["All sites", "Harbor", "Ridge", "Depot"]),
      elements.select("Status", ["Any status", "Online", "Offline", "Warning"]),
      elements.select("Sort by", ["Name", "Last seen", "Battery"]),
      elements.checkbox("Low battery only"),
      elements.button("Restart device"),
      elements.button("Update firmware"),
      elements.link("Device logs", "/logs"),
      elements.link("Network map", "/map"),
      elements.spacer(1900),
      elements.button("Export inventory"),
    ],
    cases: [
      click("iot_console", "Search devices", "Search devices for the typed sensor id"),
      click("iot_console", "Restart device", "Restart device after the firmware hang"),
      click("iot_console", "Update firmware", "Update firmware on the selected sensor"),
      click("iot_console", "Device logs", "Open Device logs for the offline unit"),
      click("iot_console", "Network map", "View the Network map for the depot"),
      click("iot_console", "Low battery only", "Filter to Low battery only"),
      fill("iot_console", "Device search", "Find sensor A-104 in Device search", "query", "A-104"),
      fill("iot_console", "Device search", "Look up gateway 7 in Device search", "query", "gateway 7", "gateway"),
      select("iot_console", "Site", "Set Site to Ridge", "site", "Ridge"),
      select("iot_console", "Status", "Choose Status Warning", "status", "Warning"),
      select("iot_console", "Sort by", "Change Sort by to Battery", "sort", "Battery"),
      blockedFill("iot_console", "Device search", "Fill Device search with the sensor label"),
      blockedSelect("iot_console", "Status", "Select the status from the incident report"),
      scrollDown("iot_console", "Scroll down to the inventory export button"),
      scrollUp("iot_console", "Scroll up to the device search box"),
      historyBack("iot_console", "Take me back to the alert page from before this console"),
      wait("iot_console", "Wait for the sensor heartbeat to update"),
    ],
  }),
);

// 11. Podcast studio.
pages.push(
  page({
    id: "podcast_studio",
    title: "Waveform podcast studio",
    build: () => [
      elements.nav([
        { name: "Episodes", href: "/episodes" },
        { name: "Guests", href: "/guests" },
        { name: "Analytics", href: "/analytics" },
      ]),
      elements.heading(1, "Episode editor"),
      elements.input("Episode title", { type: "text" }),
      elements.select("Show", ["Field Notes", "Signal Path", "After Hours"]),
      elements.select("Season", ["Season 1", "Season 2", "Season 3"]),
      elements.select("Visibility", ["Draft", "Private", "Public"]),
      elements.textarea("Show notes"),
      elements.checkbox("Publish to RSS"),
      elements.button("Upload audio"),
      elements.button("Save episode"),
      elements.link("Transcript editor", "/transcript"),
      elements.link("Cover art", "/cover"),
      elements.spacer(2100),
      elements.button("Schedule release"),
    ],
    cases: [
      click("podcast_studio", "Upload audio", "Upload audio for the new episode"),
      click("podcast_studio", "Save episode", "Save episode before leaving the editor"),
      click("podcast_studio", "Publish to RSS", "Enable Publish to RSS"),
      click("podcast_studio", "Transcript editor", "Open the Transcript editor"),
      click("podcast_studio", "Cover art", "Replace the Cover art"),
      click("podcast_studio", "Schedule release", "Schedule release for next Tuesday"),
      fill("podcast_studio", "Episode title", "Name the Episode title Harbor Lights", "title", "Harbor Lights"),
      fill("podcast_studio", "Episode title", "Use Signal Drift as the Episode title", "title", "Signal Drift", "alternate"),
      fill("podcast_studio", "Show notes", "Write the guest bio in Show notes", "notes", "Guest bio and links"),
      select("podcast_studio", "Show", "Set Show to Signal Path", "show", "Signal Path"),
      select("podcast_studio", "Season", "Choose Season 3", "season", "Season 3"),
      select("podcast_studio", "Visibility", "Change Visibility to Private", "visibility", "Private"),
      blockedFill("podcast_studio", "Episode title", "Fill Episode title from the recording sheet"),
      blockedSelect("podcast_studio", "Visibility", "Pick the visibility the sponsor wants"),
      scrollDown("podcast_studio", "Scroll down to the release schedule button"),
      scrollUp("podcast_studio", "Scroll up to the episode title"),
      historyBack("podcast_studio", "Go back to the episode list I opened before this editor"),
      wait("podcast_studio", "Wait for the audio waveform to render"),
    ],
  }),
);

// 12. Legal review.
pages.push(
  page({
    id: "legal_review",
    title: "Quill legal review",
    build: () => [
      elements.nav([
        { name: "Matters", href: "/matters" },
        { name: "Contracts", href: "/contracts" },
        { name: "Deadlines", href: "/deadlines" },
      ]),
      elements.heading(1, "Contract review"),
      elements.input("Search contracts", { type: "search" }),
      elements.button("Find contracts"),
      elements.select("Matter", ["Harbor lease", "Ridge supply", "Depot sale"]),
      elements.select("Reviewer", ["Unassigned", "Anika Rao", "Tom Bell"]),
      elements.select("Priority", ["Low", "Normal", "High"]),
      elements.textarea("Review note"),
      elements.checkbox("Flag for partner review"),
      elements.button("Approve clause"),
      elements.button("Reject clause"),
      elements.link("Redline history", "/redlines"),
      elements.link("Clause library", "/clauses"),
      elements.spacer(2000),
      elements.button("Send to counterparty"),
    ],
    cases: [
      click("legal_review", "Find contracts", "Search contracts for the vendor name"),
      click("legal_review", "Approve clause", "Approve clause 4 after review"),
      click("legal_review", "Reject clause", "Reject clause 7 as written"),
      click("legal_review", "Flag for partner review", "Flag for partner review"),
      click("legal_review", "Redline history", "Open the Redline history"),
      click("legal_review", "Clause library", "Browse the Clause library"),
      fill("legal_review", "Search contracts", "Find the Harbor lease in Search contracts", "query", "Harbor lease"),
      fill("legal_review", "Search contracts", "Look up the depot sale in Search contracts", "query", "depot sale", "sale"),
      fill("legal_review", "Review note", "Write the indemnity concern in Review note", "note", "Indemnity cap is too low"),
      select("legal_review", "Matter", "Set Matter to Ridge supply", "matter", "Ridge supply"),
      select("legal_review", "Reviewer", "Assign Reviewer Anika Rao", "reviewer", "Anika Rao"),
      select("legal_review", "Priority", "Change Priority to High", "priority", "High"),
      blockedFill("legal_review", "Review note", "Fill Review note with the partner comment"),
      blockedSelect("legal_review", "Reviewer", "Pick the reviewer from the staffing email"),
      scrollDown("legal_review", "Scroll down to the counterparty send button"),
      scrollUp("legal_review", "Scroll up to the contract search"),
      historyBack("legal_review", "Return to the matter list from before this contract"),
      wait("legal_review", "Wait for the redline comparison to finish"),
    ],
  }),
);

// 13. Travel planner.
pages.push(
  page({
    id: "travel_planner",
    title: "Waypoint travel planner",
    build: () => [
      elements.nav([
        { name: "Trips", href: "/trips" },
        { name: "Flights", href: "/flights" },
        { name: "Lodging", href: "/lodging" },
      ]),
      elements.heading(1, "Trip itinerary"),
      elements.input("Destination", { type: "search" }),
      elements.input("Travel dates", { type: "text" }),
      elements.select("Travelers", ["1 traveler", "2 travelers", "Family of four"]),
      elements.select("Budget", ["Economy", "Balanced", "Comfort"]),
      elements.select("Transport", ["Any", "Train", "Flight", "Car"]),
      elements.checkbox("Direct routes only"),
      elements.button("Build itinerary"),
      elements.link("Packing list", "/packing"),
      elements.link("Visa requirements", "/visa"),
      elements.spacer(2300),
      elements.button("Share trip"),
    ],
    cases: [
      click("travel_planner", "Build itinerary", "Build itinerary for the destination"),
      click("travel_planner", "Share trip", "Share trip with the travel companion"),
      click("travel_planner", "Packing list", "Open the Packing list"),
      click("travel_planner", "Visa requirements", "Check Visa requirements for the route"),
      click("travel_planner", "Direct routes only", "Limit the plan to Direct routes only"),
      fill("travel_planner", "Destination", "Set Destination to Lisbon", "destination", "Lisbon"),
      fill("travel_planner", "Destination", "Use Porto as the Destination", "destination", "Porto", "porto"),
      fill("travel_planner", "Travel dates", "Enter Travel dates as 2027-03-10 to 2027-03-18", "dates", "2027-03-10 to 2027-03-18"),
      select("travel_planner", "Travelers", "Set Travelers to Family of four", "travelers", "Family of four"),
      select("travel_planner", "Budget", "Choose Budget Comfort", "budget", "Comfort"),
      select("travel_planner", "Transport", "Switch Transport to Train", "transport", "Train"),
      blockedFill("travel_planner", "Destination", "Fill Destination with the city from the invite"),
      blockedSelect("travel_planner", "Budget", "Select the budget we discussed"),
      scrollDown("travel_planner", "Scroll down to the share trip button"),
      scrollUp("travel_planner", "Scroll up to the destination field"),
      historyBack("travel_planner", "Take me back to the trip list from before this plan"),
      wait("travel_planner", "Wait for the route options to load"),
    ],
  }),
);

// 14. Warehouse ops.
pages.push(
  page({
    id: "warehouse_ops",
    title: "Dock warehouse operations",
    build: () => [
      elements.nav([
        { name: "Inbound", href: "/inbound" },
        { name: "Outbound", href: "/outbound" },
        { name: "Inventory", href: "/inventory" },
      ]),
      elements.heading(1, "Dock schedule"),
      elements.input("Search orders", { type: "search" }),
      elements.button("Find orders"),
      elements.select("Dock", ["Dock 1", "Dock 2", "Dock 3"]),
      elements.select("Carrier", ["Any carrier", "Blue Line", "North Rail"]),
      elements.select("Shift", ["Morning", "Afternoon", "Night"]),
      elements.checkbox("Late arrivals only"),
      elements.button("Assign dock"),
      elements.button("Print labels"),
      elements.link("Dock map", "/map"),
      elements.link("Safety checklist", "/safety"),
      elements.spacer(2100),
      elements.button("Close shift"),
    ],
    cases: [
      click("warehouse_ops", "Find orders", "Search orders for the inbound pallet"),
      click("warehouse_ops", "Assign dock", "Assign dock 2 to the arriving truck"),
      click("warehouse_ops", "Print labels", "Print labels for the outbound boxes"),
      click("warehouse_ops", "Dock map", "Open the Dock map"),
      click("warehouse_ops", "Safety checklist", "Read the Safety checklist"),
      click("warehouse_ops", "Late arrivals only", "Filter to Late arrivals only"),
      fill("warehouse_ops", "Search orders", "Find order 8841 in Search orders", "query", "8841"),
      fill("warehouse_ops", "Search orders", "Look up pallet P-22 in Search orders", "query", "P-22", "pallet"),
      select("warehouse_ops", "Dock", "Set Dock to Dock 3", "dock", "Dock 3"),
      select("warehouse_ops", "Carrier", "Choose Carrier North Rail", "carrier", "North Rail"),
      select("warehouse_ops", "Shift", "Change Shift to Night", "shift", "Night"),
      blockedFill("warehouse_ops", "Search orders", "Fill Search orders with the bill number"),
      blockedSelect("warehouse_ops", "Dock", "Pick the dock from the dispatch call"),
      scrollDown("warehouse_ops", "Scroll down to the close shift button"),
      scrollUp("warehouse_ops", "Scroll up to the order search"),
      historyBack("warehouse_ops", "Go back to the inbound list I came from"),
      wait("warehouse_ops", "Wait for the dock availability to refresh"),
    ],
  }),
);

// 15. Photo library.
pages.push(
  page({
    id: "photo_library",
    title: "Lumen photo library",
    build: () => [
      elements.nav([
        { name: "Albums", href: "/albums" },
        { name: "People", href: "/people" },
        { name: "Shared", href: "/shared" },
      ]),
      elements.heading(1, "Photo library"),
      elements.input("Search photos", { type: "search" }),
      elements.button("Find photos"),
      elements.select("Album", ["All albums", "Harbor 2026", "Studio", "Family"]),
      elements.select("Media type", ["All media", "Photos", "Videos"]),
      elements.select("Sort by", ["Newest", "Oldest", "Largest"]),
      elements.checkbox("Favorites only"),
      elements.button("Create album"),
      elements.button("Share selection"),
      elements.link("Storage usage", "/storage"),
      elements.link("Import from device", "/import"),
      elements.spacer(1900),
      elements.button("Delete selection"),
    ],
    cases: [
      click("photo_library", "Find photos", "Run Search photos for the trip name"),
      click("photo_library", "Create album", "Create album for the harbor shoot"),
      click("photo_library", "Share selection", "Share selection with the design team"),
      click("photo_library", "Storage usage", "Open Storage usage"),
      click("photo_library", "Import from device", "Import from device after the shoot"),
      click("photo_library", "Favorites only", "Show Favorites only"),
      fill("photo_library", "Search photos", "Find sunset in Search photos", "query", "sunset"),
      fill("photo_library", "Search photos", "Look up pier at dawn in Search photos", "query", "pier at dawn", "pier"),
      select("photo_library", "Album", "Set Album to Harbor 2026", "album", "Harbor 2026"),
      select("photo_library", "Media type", "Choose Media type Videos", "media", "Videos"),
      select("photo_library", "Sort by", "Change Sort by to Largest", "sort", "Largest"),
      blockedFill("photo_library", "Search photos", "Fill Search photos with the event name"),
      blockedSelect("photo_library", "Album", "Pick the album from the client brief"),
      scrollDown("photo_library", "Scroll down to the delete selection button"),
      scrollUp("photo_library", "Scroll up to the photo search"),
      historyBack("photo_library", "Return to the album view from before this library"),
      wait("photo_library", "Wait for the thumbnails to finish loading"),
    ],
  }),
);

// 16. Event agenda.
pages.push(
  page({
    id: "event_agenda",
    title: "Summit event agenda",
    build: () => [
      elements.nav([
        { name: "Sessions", href: "/sessions" },
        { name: "Speakers", href: "/speakers" },
        { name: "Venue", href: "/venue" },
      ]),
      elements.heading(1, "Summit agenda"),
      elements.input("Search sessions", { type: "search" }),
      elements.button("Find sessions"),
      elements.button("Day one", { role: "tab" }),
      elements.button("Day two", { role: "tab" }),
      elements.select("Track", ["All tracks", "Platform", "Design", "Research"]),
      elements.select("Room", ["Main hall", "Studio A", "Studio B"]),
      elements.select("Session length", ["Any length", "30 minutes", "60 minutes"]),
      elements.checkbox("Seats available"),
      elements.button("Add to schedule"),
      elements.link("Session details", "/session/9"),
      elements.link("Speaker bios", "/bios"),
      elements.spacer(2000),
      elements.button("Download agenda"),
    ],
    cases: [
      click("event_agenda", "Find sessions", "Search sessions for the topic"),
      click("event_agenda", "Day two", "Switch to the Day two tab"),
      click("event_agenda", "Add to schedule", "Add to schedule for the keynote"),
      click("event_agenda", "Session details", "Open Session details"),
      click("event_agenda", "Speaker bios", "Read the Speaker bios"),
      click("event_agenda", "Seats available", "Show only Seats available"),
      fill("event_agenda", "Search sessions", "Find observability in Search sessions", "query", "observability"),
      fill("event_agenda", "Search sessions", "Look up design systems in Search sessions", "query", "design systems", "design"),
      select("event_agenda", "Track", "Set Track to Research", "track", "Research"),
      select("event_agenda", "Room", "Choose Room Studio A", "room", "Studio A"),
      select("event_agenda", "Session length", "Change Session length to 60 minutes", "length", "60 minutes"),
      blockedFill("event_agenda", "Search sessions", "Fill Search sessions with the speaker name"),
      blockedSelect("event_agenda", "Track", "Pick the track from the program email"),
      scrollDown("event_agenda", "Scroll down to the agenda download"),
      scrollUp("event_agenda", "Scroll up to the session search"),
      historyBack("event_agenda", "Take me back to the speaker page from before this agenda"),
      wait("event_agenda", "Wait for the session seats to update"),
    ],
  }),
);

// A second history case and a second wait case on two pages, so the set has
// more than one phrasing for each operation that takes no target.
pages[0].cases.push(
  historyBack("library_catalog", "Go back to the branch page that preceded this catalog", "branch"),
  wait("library_catalog", "Wait until the hold queue status settles", "holds"),
);
pages[6].cases.push(
  historyBack("bank_transfer", "Return to the payee page I opened before this transfer", "payee"),
  wait("bank_transfer", "Wait for the transfer fee to be calculated", "fee"),
);

// Additional value entries per page. Repeating a field with a different value
// checks that the instruction and value key decide the entry, not a single
// remembered example per field.
const extraFills = {
  library_catalog: ["Catalog search", "query", ["wildlife photography", "rare editions", "local authors"]],
  clinic_intake: ["Mobile number", "phone", ["555-0199", "555-0112", "555-0177"]],
  freight_board: ["Origin city", "origin", ["Austin", "El Paso", "Corpus Christi"]],
  museum_tickets: ["Visit date", "date", ["2027-01-09", "2027-02-14", "2027-03-21"]],
  recipe_box: ["Search recipes", "query", ["miso soup", "apple cake", "black beans"]],
  hotel_booking: ["Promo code", "promo", ["STAY3", "WEEKEND", "AUTUMN"]],
  bank_transfer: ["Memo", "memo", ["December rent", "Utilities", "School fees"]],
  crm_pipeline: ["Search deals", "query", ["Northwind", "Bluebird", "Cobalt"]],
  school_portal: ["Student name", "student", ["Kai Patel", "Lena Patel", "Omar Patel"]],
  iot_console: ["Device search", "query", ["B-208", "C-311", "D-402"]],
  podcast_studio: ["Episode title", "title", ["Night Ferry", "Low Tide", "Open Water"]],
  legal_review: ["Review note", "note", ["Term is acceptable", "Needs redline", "Escalate to partner"]],
  travel_planner: ["Destination", "destination", ["Madrid", "Seville", "Faro"]],
  warehouse_ops: ["Search orders", "query", ["9021", "7734", "6650"]],
  photo_library: ["Search photos", "query", ["harbor lights", "bridge", "market"]],
  event_agenda: ["Search sessions", "query", ["accessibility", "platform teams", "research ops"]],
};
for (const spec of pages) {
  const [target, key, values] = extraFills[spec.id];
  values.forEach((value, index) => {
    spec.cases.push(
      fill(
        spec.id,
        target,
        `Use ${value} in ${target} for the follow-up request`,
        key,
        value,
        `extra-${index + 1}`,
      ),
    );
  });
}

pages[0].cases.push(
  clickSuffix("library_catalog", "Next page", "Click Next page to see more results", "next"),
);
pages[15].cases.push(
  clickSuffix("event_agenda", "Day one", "Click the Day one tab to review earlier sessions", "day-one"),
);

const cases = pages.flatMap((spec) => spec.cases);
const names = new Set();
for (const entry of cases) {
  if (names.has(entry.name)) throw new Error(`duplicate case name: ${entry.name}`);
  names.add(entry.name);
}

mkdirSync(FIXTURES, { recursive: true });
for (const spec of pages) {
  writeFileSync(join(FIXTURES, `${spec.id}.html`), renderPage(spec));
}

const byOperation = {};
for (const entry of cases) {
  byOperation[entry.expect.operation] = (byOperation[entry.expect.operation] || 0) + 1;
}

writeFileSync(
  join(HERE, "cases.json"),
  `${JSON.stringify({ cases }, null, 2)}\n`,
);
writeFileSync(
  join(HERE, "manifest.json"),
  `${JSON.stringify(
    {
      seed: SEED,
      generated_by: "cli/evals/decide/v3/generate.mjs",
      generated_on: "2026-11-08",
      label_source:
        "Case labels were written from each page's element list and instruction intent before any decider call. No label is derived from model output.",
      pages: pages.length,
      cases: cases.length,
      blocked_cases: cases.filter((entry) => entry.expect.blocked).length,
      by_operation: byOperation,
      notes: [
        "Fixtures are rendered from the element lists in generate.mjs.",
        "Target accessible names appear exactly once per page; similar and duplicate distractor names are intentional and never labeled.",
        "Blocked cases are fill or select requests whose caller supplied no value.",
        "The seed is fixed at 20261109 for reproducibility; fixture text and labels contain no randomness.",
        "No case expects done. act has no done operation.",
      ],
    },
    null,
    2,
  )}\n`,
);

console.log(
  `wrote ${cases.length} cases across ${pages.length} pages: ${JSON.stringify(byOperation)}`,
);
