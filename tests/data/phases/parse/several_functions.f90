! Function with nested do loops
function total(x, n) result(s)
  real :: x(n), s
  integer :: n, i, j
  ! sqrt in a comment is not counted
  s = 0.0
  do i = 1, n
    do j = 1, n
      s = s + x(i)
    end do
  end do
  do while (s > 100.0)
    s = s / 2.0
  end do
  do
    exit
  end do
  if (s < 0.0) then
    s = -s
  end if
end function total

! Subroutine with a labelled loop, conditionals, array statements, a string and a call
subroutine scale(x, n, mode)
  real :: x(n)
  integer :: n, mode, k
  do 10 k = 1, n
    x(k) = sqrt(x(k))
10 continue
  if (mode - 1) 20, 30, 30
20 continue
30 continue
  select case (mode)
  case (1)
    x = x * 2.0
  case default
    x = x / 2.0
  end select
  where (x > 1.0) x = 1.0
  forall (k = 1:n) x(k) = x(k) + 1.0
  print *, 'sqrt'
  call print_values(x, n)
end subroutine scale

! Subroutine with select type and select rank
subroutine kinds(v, a)
  class(*) :: v
  real :: a(..)
#if 0 /* sqrt in a preprocessor comment is not counted */
  print *, 1.0
#endif
  select type (v)
  type is (real)
    print *, sqrt(v)
  end select
  select rank (a)
  rank (1)
    print *, sqrt(a(1))
  end select
end subroutine kinds
