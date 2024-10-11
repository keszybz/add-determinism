# Figure out if serialization of mutable objects depends on
# ref use.
ll = [ "a", "b", "c" ]
ll2 = ll
ll3 = [ "a", "b", "c" ]
