# Ocelot animation state

The shipped 1.26.60.4 ocelot movement controller reads `variable.state` without
assigning it in pack scripts. The client publishes that variable before
`pre_animation`. An unset variable reads as zero and incorrectly selects sneaking,
even when the server supplies the Sitting flag.

The native variable writer at x86-64 RVA `0x12a708e0` checks synced flags in this
order: Sneaking (1) selects 0, Sprinting (3) selects 1, Sitting (24) selects 2,
otherwise it selects 3. The binary's Build ID is
`bd1d3219f7de4bc579e187b21efb032766c7c6e2`. The shipped controller maps those
values to sneak, sprint, sit and walk respectively.

`actor-animation::ocelot` publishes that selector for the pinned ocelot constructor
and custom actors advertising that native base. Other actors keep their own
`variable.state`; authored pre-animation scripts run afterward and retain priority.
Synthetic tests cover all flag combinations, flag changes, native-base inheritance
and exclusion of cats, parrots and similarly named custom actors.

Contributor gameplay captures show an ocelot remaining on all four legs before
the fix and selecting its sitting pose afterward, matching the reported vanilla
behavior. Moving-state visual qualification remains open; flag combinations and
state refreshes are covered by the synthetic regressions.
