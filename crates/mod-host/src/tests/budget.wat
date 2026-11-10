(component
  (import "$BUDGET" (instance $budget
    (export "remaining-fuel" (func (result u64)))))
  (alias export $budget "remaining-fuel" (func $remaining-fuel))
  (core func $lower-remaining (canon lower (func $remaining-fuel)))
  (core module $code
    (import "host" "remaining" (func $remaining (result i64)))
    ;; Traps unless the first read is within (0, $ALLOWANCE] and `spin` iterations of work
    ;; later read at least `spend` less.
    (func $check (param $spin i32) (param $spend i64)
      (local $first i64) (local $i i32)
      call $remaining local.set $first
      local.get $first i64.eqz if unreachable end
      local.get $first i64.const $ALLOWANCE i64.gt_u if unreachable end
      (loop $work
        local.get $i i32.const 1 i32.add local.tee $i
        local.get $spin i32.lt_u br_if $work)
      local.get $first call $remaining i64.sub
      local.get $spend i64.lt_u if unreachable end)
    (func (export "init") i32.const 0 i64.const 0 call $check)
    (func (export "frame") i32.const 1000 i64.const 1000 call $check))
  (core instance $host (export "remaining" (func $lower-remaining)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
