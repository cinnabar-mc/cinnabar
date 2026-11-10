(component
  (import "$INPUT" (instance $input
    (type $record (record
      (field "line-x" f32) (field "line-y" f32)
      (field "pixel-x" f32) (field "pixel-y" f32)))
    (export "scroll" (type $scroll (eq $record)))
    (export "read-scroll" (func (result (result $scroll (error string)))))
    (export "reserve-scroll" (func (param "keys" (list string)) (result (result (error string)))))))
  (import "$HUD" (instance $hud
    (export "set-label" (func (param "text" string) (result (result (error string)))))))
  (alias export $input "read-scroll" (func $read))
  (alias export $input "reserve-scroll" (func $reserve))
  (alias export $hud "set-label" (func $label))
  (core module $mem
    (memory (export "memory") 1)
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add global.set $next local.get $old)
    (data (i32.const 0) "\10\00\00\00\04\00\00\00")
    (data (i32.const 16) "KeyC")
    (data (i32.const 32) "scroll")
    (data (i32.const 40) "none"))
  (core instance $mem (instantiate $mem))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-read (canon lower (func $read) (memory $memory) (realloc $realloc)))
  (core func $lower-reserve (canon lower (func $reserve) (memory $memory) (realloc $realloc)))
  (core func $lower-label (canon lower (func $label) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "memory" (memory 1))
    (import "host" "read" (func $read (param i32)))
    (import "host" "reserve" (func $reserve (param i32 i32 i32)))
    (import "host" "label" (func $label (param i32 i32 i32)))
    (func (export "init")
      i32.const 0 i32.const $CLAIM_COUNT i32.const 128 call $reserve)
    (func (export "frame")
      i32.const 64 call $read
      i32.const 64 i32.load i32.eqz
      if
        i32.const 72 f32.load f32.const 0 f32.ne
        if
          i32.const 32 i32.const 6 i32.const 128 call $label
        else
          i32.const 40 i32.const 4 i32.const 128 call $label
        end
      end
      $FRAME_END))
  (core instance $host
    (export "memory" (memory $memory))
    (export "read" (func $lower-read))
    (export "reserve" (func $lower-reserve))
    (export "label" (func $lower-label)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
