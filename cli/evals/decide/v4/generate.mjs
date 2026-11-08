#!/usr/bin/env node
// Deterministic generator for the v4 decision eval set.
//
// Every fixture is rendered from the page data below. Every case names the
// operation, the expected accessible name, and the values the caller supplies.
// Labels were written from those page definitions before any decider call and
// are never derived from model output.
//
// Run from anywhere:
//   node cli/evals/decide/v4/generate.mjs
//
// Fixture text and labels contain no randomness, so output is byte stable.

import { writeFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const SEED = 20261110;
const HERE = dirname(fileURLToPath(import.meta.url));
const FIXTURES = join(HERE, "fixtures");

const slug = (text) =>
  text
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-|-$/g, "");

const pages = [
  {
    id: "claims_portal",
    title: "Meridian claims portal",
    nav: ["Claims", "Members", "Payments", "Documents"],
    actions: [
      "Approve claim",
      "Request documents",
      "Assign adjuster",
      "Flag duplicate",
      "Export claim",
    ],
    fields: [
      ["Claim number", "claim", "CLM-4821"],
      ["Member name", "member", "Dana Ruiz"],
      ["Service date", "date", "2027-01-12"],
      ["Provider NPI", "npi", "1548392017"],
      ["Adjuster note", "note", "Review the duplicate line"],
    ],
    selects: [
      ["Status", "status", ["Open", "In review", "Approved", "Denied"], "In review"],
      ["Priority", "priority", ["Low", "Normal", "Urgent"], "Urgent"],
      ["Region", "region", ["North", "South", "West"], "West"],
      ["Plan", "plan", ["Gold", "Silver", "Bronze"], "Silver"],
    ],
    pagination: "Next page",
  },
  {
    id: "field_service",
    title: "Field service dispatch",
    nav: ["Jobs", "Technicians", "Parts", "Schedule"],
    actions: [
      "Dispatch technician",
      "Close job",
      "Order part",
      "Reschedule visit",
      "Print work order",
    ],
    fields: [
      ["Job number", "job", "FS-20831"],
      ["Customer name", "customer", "Harbor Cafe"],
      ["Site address", "address", "44 Dock Street"],
      ["Contact phone", "phone", "555-0142"],
      ["Work note", "note", "Replace the drive belt"],
    ],
    selects: [
      ["Visit window", "window", ["Morning", "Afternoon", "Evening"], "Afternoon"],
      ["Trade", "trade", ["Electrical", "Plumbing", "HVAC"], "HVAC"],
      ["Urgency", "urgency", ["Routine", "Urgent"], "Urgent"],
      ["Zone", "zone", ["East", "West", "Central"], "East"],
    ],
    pagination: "Previous page",
  },
  {
    id: "payroll_admin",
    title: "Payroll administration",
    nav: ["Runs", "Employees", "Deductions", "Reports"],
    actions: [
      "Approve payroll",
      "Hold payment",
      "Add deduction",
      "Recalculate taxes",
      "Export register",
    ],
    fields: [
      ["Pay period", "period", "2027-03"],
      ["Employee name", "employee", "Priya Nair"],
      ["Employee id", "employee_id", "E-10442"],
      ["Bank account", "bank", "8842"],
      ["Memo", "memo", "March adjustment"],
    ],
    selects: [
      ["Pay group", "group", ["Weekly", "Biweekly", "Monthly"], "Biweekly"],
      ["Cost center", "center", ["Operations", "Sales", "Support"], "Operations"],
      ["Currency", "currency", ["USD", "CAD", "EUR"], "USD"],
      ["Method", "method", ["Direct deposit", "Check"], "Direct deposit"],
    ],
    pagination: "Next page",
  },
  {
    id: "fleet_maintenance",
    title: "Fleet maintenance board",
    nav: ["Vehicles", "Work orders", "Inspections", "Fuel"],
    actions: [
      "Schedule service",
      "Mark inspected",
      "Order tires",
      "Report fault",
      "Download log",
    ],
    fields: [
      ["Vehicle id", "vehicle", "VAN-317"],
      ["Odometer", "odometer", "84210"],
      ["Driver name", "driver", "Luis Ortega"],
      ["Garage bay", "bay", "B-2"],
      ["Fault note", "fault", "Brake vibration"],
    ],
    selects: [
      ["Service type", "service", ["Oil change", "Tire rotation", "Brake service"], "Brake service"],
      ["Depot", "depot", ["North", "South", "Harbor"], "Harbor"],
      ["Job status", "job_status", ["Open", "Scheduled", "Done"], "Scheduled"],
      ["Severity", "severity", ["Low", "Medium", "High"], "High"],
    ],
    pagination: "Previous page",
  },
  {
    id: "grant_review",
    title: "Community grant review",
    nav: ["Applications", "Reviewers", "Awards", "Budget"],
    actions: [
      "Score application",
      "Request revision",
      "Approve award",
      "Assign reviewer",
      "Export scores",
    ],
    fields: [
      ["Application id", "application", "GR-7712"],
      ["Applicant name", "applicant", "Northside Arts"],
      ["Requested amount", "amount", "24000"],
      ["Review note", "note", "Strong community plan"],
      ["Reviewer name", "reviewer", "Amara Cole"],
    ],
    selects: [
      ["Track", "track", ["Arts", "Education", "Health"], "Education"],
      ["Decision", "decision", ["Pending", "Fund", "Decline"], "Fund"],
      ["Region", "region", ["Metro", "Coastal", "Inland"], "Coastal"],
      ["Cycle", "cycle", ["Spring", "Fall"], "Spring"],
    ],
    pagination: "Next page",
  },
  {
    id: "hotel_ops",
    title: "Hotel operations desk",
    nav: ["Rooms", "Housekeeping", "Maintenance", "Guests"],
    actions: [
      "Mark clean",
      "Report damage",
      "Assign room",
      "Extend stay",
      "Print key card",
    ],
    fields: [
      ["Room number", "room", "412"],
      ["Guest name", "guest", "Nora Blake"],
      ["Arrival date", "arrival", "2027-02-19"],
      ["Special request", "request", "Quiet floor"],
      ["Housekeeper name", "housekeeper", "Mei Lin"],
    ],
    selects: [
      ["Room type", "room_type", ["Standard", "Deluxe", "Suite"], "Deluxe"],
      ["Room status", "room_status", ["Clean", "Dirty", "Inspecting"], "Dirty"],
      ["Floor", "floor", ["Third", "Fourth", "Fifth"], "Fourth"],
      ["Shift", "shift", ["Morning", "Evening", "Night"], "Morning"],
    ],
    pagination: "Previous page",
  },
  {
    id: "insurance_quote",
    title: "Insurance quote workbench",
    nav: ["Quotes", "Policies", "Claims", "Customers"],
    actions: [
      "Generate quote",
      "Bind policy",
      "Add driver",
      "Compare plans",
      "Email quote",
    ],
    fields: [
      ["Quote id", "quote", "Q-55219"],
      ["Customer name", "customer", "Owen Park"],
      ["Vehicle year", "year", "2022"],
      ["Coverage amount", "coverage", "250000"],
      ["Agent note", "note", "Multi policy discount"],
    ],
    selects: [
      ["Product", "product", ["Auto", "Home", "Life"], "Auto"],
      ["Term", "term", ["Six months", "One year"], "One year"],
      ["Deductible", "deductible", ["500", "1000", "2000"], "1000"],
      ["Payment", "payment", ["Monthly", "Annual"], "Monthly"],
    ],
    pagination: "Next page",
  },
  {
    id: "lab_inventory",
    title: "Laboratory inventory",
    nav: ["Samples", "Reagents", "Equipment", "Protocols"],
    actions: [
      "Log sample",
      "Reserve reagent",
      "Flag expired",
      "Calibrate analyzer",
      "Export inventory",
    ],
    fields: [
      ["Sample id", "sample", "LAB-9914"],
      ["Reagent name", "reagent", "Buffer P2"],
      ["Lot number", "lot", "L-3381"],
      ["Storage temp", "temp", "Minus 20"],
      ["Technician name", "technician", "Ravi Shah"],
    ],
    selects: [
      ["Assay", "assay", ["PCR", "ELISA", "Sequencing"], "PCR"],
      ["Storage", "storage", ["Fridge", "Freezer", "Bench"], "Freezer"],
      ["Hazard", "hazard", ["None", "Flammable", "Biohazard"], "Biohazard"],
      ["Stock status", "stock", ["In stock", "Low", "Expired"], "Low"],
    ],
    pagination: "Previous page",
  },
  {
    id: "logistics_hub",
    title: "Logistics control tower",
    nav: ["Shipments", "Carriers", "Routes", "Exceptions"],
    actions: [
      "Book pickup",
      "Split shipment",
      "Reroute load",
      "Print labels",
      "Release hold",
    ],
    fields: [
      ["Shipment id", "shipment", "SH-44820"],
      ["Consignee name", "consignee", "Apex Retail"],
      ["Pickup city", "city", "Dayton"],
      ["Weight kg", "weight", "1240"],
      ["Handling note", "note", "Keep upright"],
    ],
    selects: [
      ["Service", "service", ["Ground", "Air", "Ocean"], "Air"],
      ["Route", "route", ["North", "South", "Express"], "Express"],
      ["Shipment status", "shipment_status", ["Ready", "Held", "In transit"], "Held"],
      ["Packaging", "packaging", ["Pallet", "Crate", "Box"], "Crate"],
    ],
    pagination: "Next page",
  },
  {
    id: "membership_admin",
    title: "Membership administration",
    nav: ["Members", "Plans", "Renewals", "Events"],
    actions: [
      "Renew membership",
      "Cancel plan",
      "Send reminder",
      "Upgrade tier",
      "Export roster",
    ],
    fields: [
      ["Member id", "member_id", "M-2081"],
      ["Member name", "member", "Talia Brooks"],
      ["Join date", "join", "2024-06-03"],
      ["Email address", "email", "talia@example.org"],
      ["Note", "note", "Annual renewal"],
    ],
    selects: [
      ["Plan", "plan", ["Basic", "Plus", "Pro"], "Plus"],
      ["Billing", "billing", ["Monthly", "Annual"], "Annual"],
      ["Member status", "member_status", ["Active", "Paused", "Lapsed"], "Paused"],
      ["Chapter", "chapter", ["Downtown", "Riverside", "North"], "Riverside"],
    ],
    pagination: "Previous page",
  },
  {
    id: "newsroom",
    title: "Newsroom desk",
    nav: ["Stories", "Authors", "Media", "Analytics"],
    actions: [
      "Publish story",
      "Save draft",
      "Request review",
      "Feature article",
      "Download assets",
    ],
    fields: [
      ["Headline", "headline", "Harbor transit plan"],
      ["Author name", "author", "Ines Duarte"],
      ["Slug", "slug", "harbor-transit-plan"],
      ["Publish date", "publish", "2027-04-02"],
      ["Summary", "summary", "New service map"],
    ],
    selects: [
      ["Section", "section", ["Local", "Business", "Culture"], "Business"],
      ["Story status", "story_status", ["Draft", "Review", "Published"], "Review"],
      ["Format", "format", ["Article", "Video", "Gallery"], "Video"],
      ["Audience", "audience", ["All", "Subscribers", "Staff"], "Subscribers"],
    ],
    pagination: "Next page",
  },
  {
    id: "pharmacy_queue",
    title: "Pharmacy queue",
    nav: ["Queue", "Patients", "Inventory", "Labels"],
    actions: [
      "Fill prescription",
      "Verify order",
      "Call patient",
      "Reprint label",
      "Flag interaction",
    ],
    fields: [
      ["Prescription number", "prescription", "RX-70218"],
      ["Patient name", "patient", "Hana Kim"],
      ["Drug name", "drug", "Metformin"],
      ["Dose", "dose", "500 mg"],
      ["Pharmacist note", "note", "Check refills"],
    ],
    selects: [
      ["Priority", "priority", ["Normal", "Urgent"], "Urgent"],
      ["Order status", "order_status", ["Queued", "Filling", "Ready"], "Filling"],
      ["Pickup", "pickup", ["Counter", "Drive through", "Delivery"], "Drive through"],
      ["Insurance", "insurance", ["Primary", "Secondary", "Cash"], "Primary"],
    ],
    pagination: "Previous page",
  },
  {
    id: "procurement",
    title: "Procurement requests",
    nav: ["Requests", "Suppliers", "Orders", "Invoices"],
    actions: [
      "Create order",
      "Approve request",
      "Request quotes",
      "Receive goods",
      "Export spend",
    ],
    fields: [
      ["Request id", "request", "PR-3307"],
      ["Supplier name", "supplier", "Baltic Supply"],
      ["Item description", "item", "Steel brackets"],
      ["Quantity", "quantity", "240"],
      ["Cost center", "center", "Facilities"],
    ],
    selects: [
      ["Category", "category", ["Hardware", "Software", "Services"], "Hardware"],
      ["Approval", "approval", ["Pending", "Approved", "Rejected"], "Approved"],
      ["Delivery", "delivery", ["Standard", "Expedited"], "Expedited"],
      ["Currency", "currency", ["USD", "EUR", "GBP"], "EUR"],
    ],
    pagination: "Next page",
  },
  {
    id: "property_listings",
    title: "Property listings",
    nav: ["Listings", "Agents", "Tours", "Offers"],
    actions: [
      "Schedule tour",
      "Submit offer",
      "Share listing",
      "Mark sold",
      "Download flyer",
    ],
    fields: [
      ["Listing id", "listing", "PL-6620"],
      ["Address", "address", "18 Cedar Lane"],
      ["Asking price", "price", "425000"],
      ["Agent name", "agent", "Marco Silva"],
      ["Feature note", "note", "Renovated kitchen"],
    ],
    selects: [
      ["Property type", "property_type", ["House", "Condo", "Townhouse"], "Townhouse"],
      ["Bedrooms", "bedrooms", ["Two", "Three", "Four"], "Three"],
      ["Listing status", "listing_status", ["Active", "Pending", "Sold"], "Pending"],
      ["Neighborhood", "neighborhood", ["Hillcrest", "Lakeview", "Old Town"], "Lakeview"],
    ],
    pagination: "Previous page",
  },
  {
    id: "recruiting",
    title: "Recruiting pipeline",
    nav: ["Candidates", "Roles", "Interviews", "Offers"],
    actions: [
      "Move to interview",
      "Reject candidate",
      "Send offer",
      "Schedule screen",
      "Export pipeline",
    ],
    fields: [
      ["Candidate name", "candidate", "Jordan Ellis"],
      ["Role title", "role", "Staff engineer"],
      ["Recruiter name", "recruiter", "Sofia Marin"],
      ["Interview date", "interview", "2027-05-11"],
      ["Feedback note", "feedback", "Strong systems design"],
    ],
    selects: [
      ["Stage", "stage", ["Screen", "Interview", "Offer"], "Interview"],
      ["Source", "source", ["Referral", "Careers page", "Agency"], "Referral"],
      ["Location", "location", ["Remote", "Austin", "London"], "Remote"],
      ["Level", "level", ["Mid", "Senior", "Staff"], "Senior"],
    ],
    pagination: "Next page",
  },
  {
    id: "reservations",
    title: "Restaurant reservations",
    nav: ["Tables", "Guests", "Queue", "Floor plan"],
    actions: [
      "Seat party",
      "Add to queue",
      "Cancel reservation",
      "Move table",
      "Print ticket",
    ],
    fields: [
      ["Reservation name", "name", "Ada Flores"],
      ["Party size", "party", "6"],
      ["Time", "time", "7:30 PM"],
      ["Phone number", "phone", "555-0188"],
      ["Note", "note", "Window table"],
    ],
    selects: [
      ["Service", "service", ["Dinner", "Lunch", "Brunch"], "Dinner"],
      ["Area", "area", ["Patio", "Dining room", "Bar"], "Patio"],
      ["Reservation status", "reservation_status", ["Booked", "Seated", "Cancelled"], "Booked"],
      ["Occasion", "occasion", ["Birthday", "Anniversary", "None"], "Birthday"],
    ],
    pagination: "Previous page",
  },
  {
    id: "retail_orders",
    title: "Retail order desk",
    nav: ["Orders", "Returns", "Products", "Customers"],
    actions: [
      "Fulfill order",
      "Refund order",
      "Print packing slip",
      "Split order",
      "Export orders",
    ],
    fields: [
      ["Order number", "order", "R-90114"],
      ["Customer name", "customer", "Eli Turner"],
      ["Shipping city", "city", "Boise"],
      ["Tracking number", "tracking", "1Z88A3"],
      ["Gift note", "note", "Happy birthday"],
    ],
    selects: [
      ["Carrier", "carrier", ["UPS", "FedEx", "USPS"], "FedEx"],
      ["Order status", "order_status", ["Paid", "Packed", "Shipped"], "Packed"],
      ["Shipping", "shipping", ["Standard", "Express", "Overnight"], "Express"],
      ["Payment", "payment", ["Card", "PayPal", "Gift card"], "Card"],
    ],
    pagination: "Next page",
  },
  {
    id: "support_desk",
    title: "Support desk queue",
    nav: ["Tickets", "Queues", "Agents", "Knowledge"],
    actions: [
      "Assign ticket",
      "Escalate ticket",
      "Close ticket",
      "Add tag",
      "Export queue",
    ],
    fields: [
      ["Ticket id", "ticket", "SD-5510"],
      ["Subject", "subject", "Login loop"],
      ["Requester name", "requester", "Mina Patel"],
      ["Agent name", "agent", "Chris Doyle"],
      ["Resolution note", "resolution", "Cleared the cache"],
    ],
    selects: [
      ["Priority", "priority", ["Low", "Normal", "High"], "High"],
      ["Queue", "queue", ["Billing", "Technical", "Account"], "Technical"],
      ["Ticket status", "ticket_status", ["New", "Open", "Resolved"], "Open"],
      ["Channel", "channel", ["Email", "Chat", "Phone"], "Chat"],
    ],
    pagination: "Previous page",
  },
  {
    id: "survey_builder",
    title: "Survey builder",
    nav: ["Surveys", "Questions", "Responses", "Reports"],
    actions: [
      "Add question",
      "Publish survey",
      "Preview survey",
      "Duplicate survey",
      "Export responses",
    ],
    fields: [
      ["Survey title", "title", "Spring feedback"],
      ["Question text", "question", "How was the event"],
      ["Response limit", "limit", "500"],
      ["Closing date", "closing", "2027-06-01"],
      ["Owner name", "owner", "Ruth Meyer"],
    ],
    selects: [
      ["Question type", "question_type", ["Text", "Choice", "Rating"], "Rating"],
      ["Audience", "audience", ["Customers", "Employees", "Students"], "Employees"],
      ["Survey status", "survey_status", ["Draft", "Live", "Closed"], "Live"],
      ["Language", "language", ["English", "Spanish", "French"], "Spanish"],
    ],
    pagination: "Next page",
  },
  {
    id: "timesheet",
    title: "Weekly timesheet",
    nav: ["Week", "Projects", "Approvals", "Reports"],
    actions: [
      "Submit timesheet",
      "Add entry",
      "Approve hours",
      "Copy last week",
      "Export hours",
    ],
    fields: [
      ["Employee name", "employee", "Noah Grant"],
      ["Week ending", "week", "2027-03-14"],
      ["Project name", "project", "Atlas migration"],
      ["Hours", "hours", "38"],
      ["Entry note", "note", "Client workshops"],
    ],
    selects: [
      ["Project", "project_select", ["Atlas", "Beacon", "Cedar"], "Beacon"],
      ["Task", "task", ["Design", "Build", "Test"], "Build"],
      ["Timesheet status", "timesheet_status", ["Draft", "Submitted", "Approved"], "Submitted"],
      ["Billable", "billable", ["Yes", "No"], "Yes"],
    ],
    pagination: "Previous page",
  },
  {
    id: "training_academy",
    title: "Training academy",
    nav: ["Courses", "Learners", "Assignments", "Certificates"],
    actions: [
      "Enroll learner",
      "Mark attended",
      "Assign course",
      "Issue certificate",
      "Export progress",
    ],
    fields: [
      ["Learner name", "learner", "Grace Liu"],
      ["Course title", "course", "Data foundations"],
      ["Cohort name", "cohort", "Spring 2027"],
      ["End date", "completion", "2027-04-30"],
      ["Instructor name", "instructor", "Ben Okafor"],
    ],
    selects: [
      ["Track", "track", ["Data", "Security", "Leadership"], "Security"],
      ["Level", "level", ["Intro", "Intermediate", "Advanced"], "Intermediate"],
      ["Course status", "course_status", ["Not started", "In progress", "Complete"], "In progress"],
      ["Format", "format", ["Live", "Self paced"], "Self paced"],
    ],
    pagination: "Next page",
  },
  {
    id: "vendor_portal",
    title: "Vendor portal",
    nav: ["Vendors", "Contracts", "Invoices", "Compliance"],
    actions: [
      "Approve vendor",
      "Upload contract",
      "Dispute invoice",
      "Request documents",
      "Export vendors",
    ],
    fields: [
      ["Vendor name", "vendor", "Summit Freight"],
      ["Contact name", "contact", "Iris Chen"],
      ["Tax id", "tax", "88-2047113"],
      ["Contract end", "contract_end", "2027-12-31"],
      ["Compliance note", "note", "Insurance current"],
    ],
    selects: [
      ["Category", "category", ["Logistics", "Software", "Facilities"], "Logistics"],
      ["Vendor status", "vendor_status", ["Prospect", "Active", "Suspended"], "Active"],
      ["Review", "review", ["Annual", "Biennial"], "Annual"],
      ["Region", "region", ["Domestic", "International"], "Domestic"],
    ],
    pagination: "Previous page",
  },
];

// Thirteen pages carry the two blocked labels. The rest carry extra value
// cases. That keeps blocked cases near six percent of the set, so a fully
// correct policy still covers well over ninety percent of decisions.
const BLOCKED_PAGES = new Set([
  "claims_portal",
  "field_service",
  "fleet_maintenance",
  "grant_review",
  "insurance_quote",
  "lab_inventory",
  "membership_admin",
  "newsroom",
  "procurement",
  "property_listings",
  "reservations",
  "retail_orders",
  "survey_builder",
]);

const clickPhrases = [
  (name) => `Press ${name} to apply it to the current selection`,
  (name) => `Run ${name} for this week's records`,
  (name) => `Open ${name} for the highlighted record`,
  (name) => `Start ${name} now`,
  (name) => `Use ${name} on the selected rows`,
];

const fillPhrases = [
  (name, value) => `Enter ${value} in ${name}`,
  (name, value) => `Put ${value} in the ${name} field`,
  (name, value) => `Type ${value} into ${name}`,
  (name, value) => `Write ${value} in ${name}`,
  (name, value) => `Add ${value} to ${name}`,
];

const selectPhrases = [
  (name, value) => `Set ${name} to ${value}`,
  (name, value) => `Choose ${value} for ${name}`,
  (name, value) => `Change ${name} to ${value}`,
  (name, value) => `Pick ${value} in the ${name} dropdown`,
];

function caseName(pageId, operation, target, suffix) {
  return `${pageId}-${operation}-${slug(target)}${suffix ? `-${suffix}` : ""}`;
}

function targetCase(pageId, operation, target, instruction, values, suffix) {
  const entry = {
    name: caseName(pageId, operation, target, suffix),
    fixture: `${pageId}.html`,
    instruction,
  };
  if (values) entry.values = values;
  entry.expect = { operation, target_name: target };
  return entry;
}

function plainCase(pageId, operation, instruction, hint) {
  return {
    name: `${pageId}-${operation}-${hint}`,
    fixture: `${pageId}.html`,
    instruction,
    expect: { operation },
  };
}

function blockedCase(pageId, operation, target, instruction) {
  return {
    name: caseName(pageId, operation, target, "blocked"),
    fixture: `${pageId}.html`,
    instruction,
    expect: { operation, target_name: target, blocked: true },
  };
}

function renderPage(spec) {
  const nav = spec.nav
    .map((name) => `      <a href="/${slug(name)}">${name}</a>`)
    .join("\n");
  const fields = spec.fields
    .map(
      ([name, key, value], index) =>
        `        <label for="${spec.id}-f${index}">${name}</label>\n` +
        `        <input id="${spec.id}-f${index}" type="text" data-key="${key}" value="${value}">`,
    )
    .join("\n");
  const selects = spec.selects
    .map(
      ([name, key, options, value], index) =>
        `        <label for="${spec.id}-s${index}">${name}</label>\n` +
        `        <select id="${spec.id}-s${index}" data-key="${key}">\n` +
        options
          .map(
            (option) =>
              `          <option${option === value ? " selected" : ""}>${option}</option>`,
          )
          .join("\n") +
        `\n        </select>`,
    )
    .join("\n");
  const actions = spec.actions
    .map((name) => `      <button type="button">${name}</button>`)
    .join("\n");
  const back = `Back to ${spec.nav[0]}`;
  const body = `    <header>
      <h1>${spec.title}</h1>
      <nav>
${nav}
      </nav>
    </header>
    <main>
      <section>
        <h2>Record details</h2>
        <form>
${fields}
${selects}
        </form>
      </section>
      <section>
        <h2>Available actions</h2>
${actions}
        <p><a href="/">${back}</a></p>
      </section>
      <section>
        <h2>Results</h2>
        <p>Totals are refreshed after each change.</p>
        <p><a href="/page/2">${spec.pagination}</a></p>
      </section>
    </main>
    <div style="height: 1400px"> </div>
    <footer>
      <p>Footer totals and audit stamp</p>
    </footer>`;
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

const cases = [];
const allNames = new Set();

for (const spec of pages) {
  const names = [
    ...spec.nav,
    ...spec.actions,
    ...spec.fields.map(([name]) => name),
    ...spec.selects.map(([name]) => name),
    spec.pagination,
    `Back to ${spec.nav[0]}`,
  ];
  if (new Set(names).size !== names.length) {
    throw new Error(`${spec.id} has duplicate accessible names`);
  }

  spec.actions.forEach((action, index) => {
    cases.push(
      targetCase(
        spec.id,
        "click",
        action,
        clickPhrases[index % clickPhrases.length](action),
      ),
    );
  });
  cases.push(
    targetCase(spec.id, "click", spec.nav[0], `Open the ${spec.nav[0]} page`, undefined, "nav"),
  );
  cases.push(
    targetCase(
      spec.id,
      "click",
      spec.pagination,
      `Go to the ${spec.pagination} of results`,
      undefined,
      "pagination",
    ),
  );

  const blocked = BLOCKED_PAGES.has(spec.id);
  const fieldCount = blocked ? 4 : 5;
  for (let index = 0; index < fieldCount; index += 1) {
    const [name, key, value] = spec.fields[index];
    cases.push(
      targetCase(
        spec.id,
        "fill",
        name,
        fillPhrases[index % fillPhrases.length](name, value),
        { [key]: value },
      ),
    );
  }
  if (blocked) {
    const [name] = spec.fields[4];
    cases.push(
      blockedCase(
        spec.id,
        "fill",
        name,
        `Fill ${name} with the reference from the email`,
      ),
    );
  }

  const selectCount = blocked ? 3 : 4;
  for (let index = 0; index < selectCount; index += 1) {
    const [name, key, , value] = spec.selects[index];
    cases.push(
      targetCase(
        spec.id,
        "select",
        name,
        selectPhrases[index % selectPhrases.length](name, value),
        { [key]: value },
      ),
    );
  }
  if (blocked) {
    const [name] = spec.selects[3];
    cases.push(
      blockedCase(
        spec.id,
        "select",
        name,
        `Choose the ${name} from the intake form`,
      ),
    );
  }

  cases.push(
    plainCase(spec.id, "scroll_down", "Scroll down to see the footer totals", "footer"),
  );
  cases.push(
    plainCase(spec.id, "scroll_up", "Scroll up toward the page heading", "heading"),
  );
  cases.push(
    plainCase(
      spec.id,
      "back",
      "Go back to the page that opened before this one",
      "history",
    ),
  );
  cases.push(
    plainCase(spec.id, "wait", "Wait for the page to settle after loading", "settle"),
  );
}

for (const entry of cases) {
  if (allNames.has(entry.name)) {
    throw new Error(`duplicate case name: ${entry.name}`);
  }
  allNames.add(entry.name);
}

mkdirSync(FIXTURES, { recursive: true });
for (const spec of pages) {
  writeFileSync(join(FIXTURES, `${spec.id}.html`), renderPage(spec));
}

const byOperation = {};
for (const entry of cases) {
  byOperation[entry.expect.operation] = (byOperation[entry.expect.operation] || 0) + 1;
}

writeFileSync(join(HERE, "cases.json"), `${JSON.stringify({ cases }, null, 2)}\n`);
writeFileSync(
  join(HERE, "manifest.json"),
  `${JSON.stringify(
    {
      seed: SEED,
      generated_by: "cli/evals/decide/v4/generate.mjs",
      generated_on: "2026-11-08",
      label_source:
        "Case labels were written from each page's element list and instruction intent before any decider call. No label is derived from model output.",
      pages: pages.length,
      cases: cases.length,
      blocked_cases: cases.filter((entry) => entry.expect.blocked).length,
      by_operation: byOperation,
      notes: [
        "Fixtures are rendered from the page data in generate.mjs.",
        "Accessible names are unique per page, so every expected target appears exactly once.",
        "Blocked cases are fill or select requests whose caller supplied no value.",
        "Thirteen pages carry the two blocked labels; the other nine carry value cases.",
        "No case is labeled destructive. These fixtures have no destructive actions.",
        "The seed is fixed at 20261110 for reproducibility; fixture text and labels contain no randomness.",
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
