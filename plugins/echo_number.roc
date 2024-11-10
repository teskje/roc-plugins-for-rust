#[plugin] echoNumber : U64 -> Str

echoNumber : U64 -> Str
echoNumber = \n -> "The number is $(Num.toStr n)"
