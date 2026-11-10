(component
  (import "$PRESENTATION" (instance $presentation
    (type $r (record (field "r" f32) (field "g" f32) (field "b" f32) (field "a" f32)))
    (export "rgba" (type $rgba (eq $r)))
    (type $a (record (field "text" string) (field "action" string)))
    (export "chat-action" (type $chat-action (eq $a)))
    (type $tt (variant (case "every-actor") (case "actor" u64)))
    (export "tint-target" (type $tint-target (eq $tt)))
    (type $ts (enum "hurt" "not-hurt"))
    (export "tint-state" (type $tint-state (eq $ts)))
    (type $t (record (field "target" $tint-target) (field "state" $tint-state)
      (field "body" (option $rgba)) (field "armor" (option $rgba))))
    (export "actor-tint" (type $actor-tint (eq $t)))
    (export "set-actor-tints" (func (param "tints" (list $actor-tint))
      (result (result (error string)))))
    (export "set-chat-actions" (func (param "actions" (list $chat-action))
      (result (result (error string)))))))
  (alias export $presentation "set-actor-tints" (func $set-tints))
  (alias export $presentation "set-chat-actions" (func $set-actions))
  (core module $memory-module
    (memory (export "memory") 1)
    (data (i32.const 0) "$ACTION")
    (global $next (mut i32) (i32.const 4096))
    (func (export "realloc") (param i32 i32 i32 i32) (result i32)
      (local $old i32)
      global.get $next local.get 2 i32.add i32.const 1 i32.sub
      i32.const 0 local.get 2 i32.sub i32.and local.tee $old
      local.get 3 i32.add global.set $next local.get $old))
  (core instance $mem (instantiate $memory-module))
  (alias core export $mem "memory" (core memory $memory))
  (alias core export $mem "realloc" (core func $realloc))
  (core func $lower-tints (canon lower (func $set-tints) (memory $memory) (realloc $realloc)))
  (core func $lower-actions (canon lower (func $set-actions) (memory $memory) (realloc $realloc)))
  (core module $code
    (import "host" "tints" (func $tints (param i32 i32 i32)))
    (import "host" "actions" (func $actions (param i32 i32 i32)))
    (import "host" "memory" (memory 1))
    (func (export "init"))
    (func (export "frame")
      ;; One every-actor hurt tint at 1024: the target and state stay zero, the body colour is
      ;; some at 1044 with its channels from 1048, and the armour colour at 1064 stays none.
      i32.const 1044 i32.const 1 i32.store8
      i32.const 1048 f32.const $RED f32.store
      i32.const 1060 f32.const 1 f32.store
      i32.const 1024 i32.const 1 i32.const 512 call $tints
      ;; One chat action whose text and id are both the probe's action name.
      i32.const 256 i32.const 0 i32.store
      i32.const 260 i32.const $ACTION_LENGTH i32.store
      i32.const 264 i32.const 0 i32.store
      i32.const 268 i32.const $ACTION_LENGTH i32.store
      i32.const 256 i32.const 1 i32.const 512 call $actions))
  (core instance $run (instantiate $code
    (with "host" (instance
      (export "tints" (func $lower-tints))
      (export "actions" (func $lower-actions))
      (export "memory" (memory $memory))))))
  (func (export "init") (canon lift (core func $run "init")))
  (func (export "frame") (canon lift (core func $run "frame"))))
