(component
  (import "cinnabar:extension/input@0.1.0" (instance $input
    (type $e (record (field "id" string) (field "value" f32)))
    (export "event" (type $event (eq $e)))
    (type $c (record (field "seconds" f32) (field "focused" bool)
      (field "gameplay" bool) (field "panel-open" bool)
      (field "keys-pressed" (list string)) (field "keys-held" (list string))
      $ADMITTED
      (field "events" (list $event)) (field "pointer" (option (tuple f32 f32)))
      (field "wheel" f32)))
    (export "controls" (type $controls (eq $c)))
    (type $s (record (field "keys-pressed" (option (list string)))
      (field "keys-held" (option (list string))) $SELECT_ADMITTED (field "events" bool)))
    (export "selection" (type $selection (eq $s)))
    (export "read-controls" (func (result (result $controls (error string)))))
    (export "read-selected-controls" (func (param "selection" $selection)
      (result (result $controls (error string)))))
    (export "reserve-keys" (func (param "keys" (list string)) (result (result (error string)))))
    (export "demo-pressed" (func (result bool)))))
  (alias export $input "read-controls" (func $read))
  (alias export $input "read-selected-controls" (func $selected))
  (alias export $input "reserve-keys" (func $reserve))
  (alias export $input "demo-pressed" (func $demo))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.get 2 i32.const 1 i32.sub i32.add
      i32.const 0 local.get 2 i32.sub i32.and local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-read (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core func $lower-selected (canon lower (func $selected) (memory $memory) (realloc $realloc)))
  (core func $lower-reserve (canon lower (func $reserve) (memory $memory) (realloc $realloc)))
  (core func $lower-demo (canon lower (func $demo)))
  (core module $code
    (import "host" "read" (func $read (param i32)))
    (import "host" "selected" (func $selected (param i32 i32 i32 i32 i32 i32 $SELECT_PARAMS i32 i32)))
    (import "host" "reserve" (func $reserve (param i32 i32 i32)))
    (import "host" "demo" (func $demo (result i32)))
    (import "host" "memory" (memory 1))
    (data (i32.const 512) "MouseLeft")
    (func (export "init"))
    (func (export "frame")
      i32.const 1024 i32.const 512 i32.store
      i32.const 1028 i32.const 9 i32.store
      i32.const 128 call $read
      i32.const 128 i32.load8_u i32.const $DENIED i32.ne if unreachable end
      $FULL_CHECK
      i32.const 0 i32.const 0 i32.const 0
      i32.const 1 i32.const 1024 i32.const 1
      $SELECT_ARGS i32.const 0 i32.const 256 call $selected
      i32.const 256 i32.load8_u i32.const $DENIED i32.ne if unreachable end
      $SELECT_CHECK
      i32.const 1024 i32.const 1 i32.const 384 call $reserve
      i32.const 384 i32.load8_u i32.const $DENIED i32.ne if unreachable end
      call $demo i32.const 1 i32.ne if unreachable end))
  (core instance $host
    (export "read" (func $lower-read))
    (export "selected" (func $lower-selected))
    (export "reserve" (func $lower-reserve))
    (export "demo" (func $lower-demo))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
