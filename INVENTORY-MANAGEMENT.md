# Inventory visits

The planner can schedule vendor and mailbox visits independently under **Inventory
visits**. Both options are disabled by default, preserving existing planning behavior.
For each enabled service, the defaults are:

- Minimum: 50 estimated mob kills since its last visit.
- Maximum: 100 estimated mob kills since its last visit.
- Visit duration: 30 seconds, separate from travel time.

All three values are editable for each service. At minimum, the planner considers a
convenient stop. At maximum, it commits to travelling there at the next planner action
boundary; maximum is not an arrival deadline. Completing a visit resets only that
service's counter.

Quest objectives, quest-start item drops, travel farming, grinding and whole-run
dungeon estimates count. Fractional expected kills are retained; XP bonuses do not
multiply the count. Long grinding is split at thresholds, while a whole quest action
can overshoot one. Counts are planning estimates, not live bag-space measurements.

## Dungeons

Enabled services are visited before each dungeon, after any level-gating grind.
If a maximum is reached during a dungeon, the route completes the run and exits before
travelling to that service. Dungeon-exit progress survives addon reloads.

## Locations and guide compatibility

Locations come from each edition's bundled data. Vendors must be friendly to the
character's faction. Mailboxes must have valid world coordinates and be near a
friendly vendor. Custom maps, including the Skyborne starter island, are supported.
An unreachable mandatory visit invalidates the route; construction stops instead
of retrying the same action indefinitely.

Newly generated routes contain manual vendor/mailbox tasks and a dungeon-exit gate.
The addon requires explicit completion of each service task; it does not automatically
sell, repair or send mail. Addon guide format 2 declares the new step types so older
addons ask for an update. The updated addon continues to read older guide formats.
The app's saved-route format is 3; older saved routes migrate without adding visits.
Generate a new route to apply inventory scheduling to an existing guide.

## Verification

Regression tests cover independent thresholds and durations, fractional kills,
travel and dungeon estimates, dungeon entry/exit ordering, saved-guide editing,
and bounded failure when mandatory service locations are unreachable. Real-data
Skyborne tests cover both factions, all four service combinations and both greedy
and region construction methods. The addon smoke fixture checks manual service
completion and dungeon-exit persistence, plus current/older/newer import formats.

The feature uses the shared planner and configuration form for desktop and web.
Automated tests use the bundled data and stubbed WoW API; live gameplay still needs
manual verification.
