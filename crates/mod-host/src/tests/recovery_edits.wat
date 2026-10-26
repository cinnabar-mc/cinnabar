(component
  (import "$INPUT" (instance $input
    (type $text (record (field "id" string) (field "value" string)))
    (export "text-event" (type $t (eq $text)))
    (type $key (record (field "id" string) (field "key" string)
      (field "shift" bool) (field "control" bool) (field "alt" bool) (field "super-key" bool)))
    (export "field-key" (type $k (eq $key)))
    (type $field (variant (case "change" $t) (case "submit" $t) (case "key" $k)))
    (export "field-event" (type $f (eq $field)))
    (export "demo-pressed" (func (result bool)))
    (export "read-text-events" (func (result (result (list $t) (error string)))))
    (export "read-field-events" (func (result (result (list $f) (error string)))))))
  (import "$SETTINGS" (instance $settings
    (export "save" (func (param "json" string) (result (result (error string)))))))
  (import "$HUD" (instance $hud
    (export "set-label" (func (param "text" string) (result (result (error string)))))))
  (import "$PANEL_INTERFACE" (instance $panel
    (export "set-content" (func (param "json" string) (result (result (error string)))))))
  (alias export $input "demo-pressed" (func $pressed))
  (alias export $input "read-text-events" (func $text))
  (alias export $input "read-field-events" (func $field))
  (alias export $settings "save" (func $save))
  (alias export $hud "set-label" (func $label))
  (alias export $panel "set-content" (func $panel))
  (core module $memory-module
    (memory (export "memory") 4)
    (global $next (mut i32) (i32.const 4096))
    ;; Keeps returned lists and their strings in separate allocations.
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.tee $old
      local.get 3 i32.add i32.const 7 i32.add i32.const -8 i32.and
      global.set $next local.get $old)
    (data (i32.const 0) "{\22v\22:0}")
    (data (i32.const 1024) "$PANEL_JSON"))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-text (canon lower (func $text) (memory $memory) (realloc $realloc)))
  (core func $lower-field (canon lower (func $field) (memory $memory) (realloc $realloc)))
  (core func $lower-save (canon lower (func $save) (memory $memory) (realloc $realloc)))
  (core func $lower-label (canon lower (func $label) (memory $memory) (realloc $realloc)))
  (core func $lower-pressed (canon lower (func $pressed)))
  (core func $lower-panel (canon lower (func $panel) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "memory" (memory 4))
    (import "host" "text" (func $text (param i32)))
    (import "host" "field" (func $field (param i32)))
    (import "host" "save" (func $save (param i32 i32 i32)))
    (import "host" "label" (func $label (param i32 i32 i32)))
    (import "host" "pressed" (func $pressed (result i32)))
    (import "host" "panel" (func $panel (param i32 i32 i32)))
    ;; Applies value data to both guest-owned presentation and persistent preferences.
    (func $apply (param $pointer i32) (param $length i32)
      local.get $pointer local.get $length i32.const 768 call $save
      i32.const 768 i32.load8_u if unreachable end
      local.get $pointer local.get $length i32.const 800 call $label
      i32.const 800 i32.load8_u if unreachable end)
    (func (export "init")
      i32.const 1024 i32.const $PANEL_LENGTH i32.const 832 call $panel
      i32.const 832 i32.load8_u if unreachable end
      i32.const 0 i32.const 7 i32.const 800 call $label)
    (func (export "frame") (local $pointer i32) (local $remaining i32)
      i32.const 512 call $text
      i32.const 512 i32.load8_u if unreachable end
      i32.const 516 i32.load local.set $pointer
      i32.const 520 i32.load local.set $remaining
      block $text-done loop $texts
        local.get $remaining i32.eqz br_if $text-done
        local.get $pointer i32.load offset=8
        local.get $pointer i32.load offset=12 call $apply
        local.get $pointer i32.const 16 i32.add local.set $pointer
        local.get $remaining i32.const 1 i32.sub local.set $remaining
        br $texts
      end end
      i32.const 544 call $field
      i32.const 544 i32.load8_u if unreachable end
      i32.const 548 i32.load local.set $pointer
      i32.const 552 i32.load local.set $remaining
      block $field-done loop $fields
        local.get $remaining i32.eqz br_if $field-done
        ;; A replacement may receive values, but never replay submit or navigation edges.
        call $pressed i32.eqz if
          local.get $pointer i32.load8_u if unreachable end
        end
        local.get $pointer i32.load8_u i32.const 2 i32.lt_u if
          local.get $pointer i32.load offset=12
          local.get $pointer i32.load offset=16 call $apply
        end
        local.get $pointer i32.const 24 i32.add local.set $pointer
        local.get $remaining i32.const 1 i32.sub local.set $remaining
        br $fields
      end end
      call $pressed if (loop $forever br $forever) end))
  (core instance $host
    (export "memory" (memory $memory))
    (export "text" (func $lower-text))
    (export "field" (func $lower-field))
    (export "save" (func $lower-save))
    (export "label" (func $lower-label))
    (export "panel" (func $lower-panel))
    (export "pressed" (func $lower-pressed)))
  (core instance $run (instantiate $code (with "host" (instance $host))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
