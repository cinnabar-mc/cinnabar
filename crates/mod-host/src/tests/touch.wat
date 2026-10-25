(component
  (import "$TOUCH" (instance $touch
    (type $a (enum "top-left" "top-center" "top-right" "left" "center" "right" "bottom-left" "bottom-center" "bottom-right"))
    (export "anchor" (type $anchor (eq $a)))
    (type $p (record (field "x" f32) (field "y" f32)))
    (export "point" (type $point (eq $p)))
    (type $b (record (field "id" string) (field "label" string) (field "anchor" $anchor) (field "offset" $point) (field "size" $point)))
    (export "button" (type $button (eq $b)))
    (export "set-buttons" (func (param "buttons" (list $button)) (result (result (error string)))))
    (export "read-presses" (func (result (result (list string) (error string)))))))
  (alias export $touch "set-buttons" (func $set))
  (alias export $touch "read-presses" (func $read))
  (core module $memory-module
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 32768))
    ;; A bump allocator, so a returned list and its strings never overlap.
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $at i32)
      global.get $next local.set $at
      global.get $next local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and global.set $next
      local.get $at))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-set (canon lower (func $set) (memory $memory) (realloc $realloc)))
  (core func $lower-read (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "set" (func $set (param i32 i32 i32)))
    (import "host" "read" (func $read (param i32)))
    (import "host" "memory" (memory 1))
    (data (i32.const 0) "demo-menu")
    (data (i32.const 16) "Menu")
    ;; One button record: id, label, anchor, offset and size.
    (func $buttons
      i32.const 1024 i32.const 0 i32.store
      i32.const 1028 i32.const 9 i32.store
      i32.const 1032 i32.const 16 i32.store
      i32.const 1036 i32.const 4 i32.store
      i32.const 1040 i32.const 2 i32.store8
      i32.const 1044 f32.const 4 f32.store
      i32.const 1048 f32.const 24 f32.store
      i32.const 1052 f32.const 36 f32.store
      i32.const 1056 f32.const 18 f32.store
      i32.const 1024 i32.const 1 i32.const 16384 call $set
      i32.const 16384 i32.load8_u i32.const $ERROR i32.ne if unreachable end)
    ;; Removes the buttons once a frame reads a tap.
    (func $presses
      i32.const 16384 call $read
      i32.const 16384 i32.load8_u i32.const $ERROR i32.ne if unreachable end
      i32.const 16384 i32.load8_u i32.eqz
      i32.const 16392 i32.load i32.const 0 i32.gt_u
      i32.and
      if i32.const 0 i32.const 0 i32.const 16400 call $set end)
    (func (export "init") $INIT)
    (func (export "frame") $FRAME))
  (core instance $host
    (export "set" (func $lower-set))
    (export "read" (func $lower-read))
    (export "memory" (memory $memory)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
