(component
  (import "$PLAYER_STATE" (instance $player-state
    (type $k (enum "attack-admitted" "hurt" "death" "totem"))
    (export "combat-kind" (type $kind (eq $k)))
    (type $e (record
      (field "sequence" u64) (field "session" u64) (field "dimension" s32) (field "kind" $kind)
      (field "local" bool) (field "player" bool) (field "runtime-id" u64)
      (field "distance" (option f32)) (field "position" (tuple f32 f32 f32))))
    (export "combat-event" (type $event (eq $e)))
    (export "read-combat" (func (param "after" u64) (param "limit" u32)
      (result (result (list $event) (error string)))))))
  (alias export $player-state "read-combat" (func $read))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "read" (func $read (param i64 i32 i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      i64.const 0 i32.const 4 i32.const 256 call $read
      i32.const 256 i32.load8_u if unreachable end
      i32.const 264 i32.load if unreachable end))
  (core instance $host
    (export "read" (func $lower))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
