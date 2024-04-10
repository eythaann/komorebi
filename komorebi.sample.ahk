#Requires AutoHotkey v2.0
#SingleInstance Force
#Include komorebic.lib.ahk

; Focus windows
!a::Focus("left")
!s::Focus("down")
!w::Focus("up")
!d::Focus("right")

!+q::CycleFocus("previous")
!q::CycleFocus("next")

; Move windows
#+a::Move("left")
#+s::Move("down")
#+w::Move("up")
#+d::Move("right")

#+Enter::Promote()

#+x::FlipLayout("horizontal")
#+z::FlipLayout("vertical")

; Stack windows
#a::Stack("left")
#d::Stack("right")
#w::Stack("up")
#s::Stack("down")
#;::Unstack()

#+q::CycleStack("previous")
#q::CycleStack("next")

; Resize
!=::ResizeAxis("horizontal", "increase")
!-::ResizeAxis("horizontal", "decrease")
!+=::ResizeAxis("vertical", "increase")
!+-::ResizeAxis("vertical", "decrease")

; Manipulate windows
#f::ToggleFloat()
#m::ToggleMonocle()

; Window manager options
#+p::TogglePause()

; Workspaces
!1::FocusWorkspace(0)
!2::FocusWorkspace(1)
!3::FocusWorkspace(2)
!4::FocusWorkspace(3)
!5::FocusWorkspace(4)
!6::FocusWorkspace(5)
!7::FocusWorkspace(6)
!8::FocusWorkspace(7)
!9::FocusWorkspace(8)

; Move windows across workspaces
!+1::MoveToWorkspace(0)
!+2::MoveToWorkspace(1)
!+3::MoveToWorkspace(2)
!+4::MoveToWorkspace(3)
!+5::MoveToWorkspace(4)
!+6::MoveToWorkspace(5)
!+7::MoveToWorkspace(6)
!+8::MoveToWorkspace(7)
!+9::MoveToWorkspace(8)

; Send windows across workspaces
#+1::SendToWorkspace(0)
#+2::SendToWorkspace(1)
#+3::SendToWorkspace(2)
#+4::SendToWorkspace(3)
#+5::SendToWorkspace(4)
#+6::SendToWorkspace(5)
#+7::SendToWorkspace(6)
#+8::SendToWorkspace(7)
#+9::SendToWorkspace(8)
