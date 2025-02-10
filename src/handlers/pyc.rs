/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{bail, Context, Result};
use log::{debug, warn};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::fs::File;
use std::hash::{Hash, Hasher};
use std::io::{self, Write};
use std::ops::Deref;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str;
use std::time;

use num_bigint_dig::{BigInt, ToBigInt};
use num_integer::Integer;
use num_traits::cast::ToPrimitive;
use num_traits::{Signed, Zero};

use crate::config;
use crate::handlers::{InputOutputHelper, unwrap_os_string};

const PYC_MAGIC: &[u8] = &[0x0D, 0x0A];
const PYLONG_MARSHAL_SHIFT: i32 = 15;
const FLAG_REF_BIT: u8 = 0x1 << 7;

const TRACE: bool = false;

pub fn pyc_python_version(buf: &[u8; 4]) -> Result<((u32, u32), usize)> {
    // https://github.com/python/cpython/blob/main/Include/internal/pycore_magic_number.h#L32
    //
    //     Python 1.5:   20121
    //     Python 1.5.1: 20121
    //     Python 1.5.2: 20121
    //     Python 1.6:   50428
    //     Python 2.0:   50823
    //     Python 2.0.1: 50823
    //     Python 2.1:   60202
    //     Python 2.1.1: 60202
    //     Python 2.1.2: 60202
    //     Python 2.2:   60717
    //     Python 2.3a0: 62011
    //     Python 2.3a0: 62021
    //     Python 2.3a0: 62011 (!)
    //     Python 2.4a0: 62041
    //     Python 2.4a3: 62051
    //     Python 2.4b1: 62061
    //     Python 2.5a0: 62071
    //     Python 2.5a0: 62081 (ast-branch)
    //     Python 2.5a0: 62091 (with)
    //     Python 2.5a0: 62092 (changed WITH_CLEANUP opcode)
    //     Python 2.5b3: 62101 (fix wrong code: for x, in ...)
    //     Python 2.5b3: 62111 (fix wrong code: x += yield)
    //     Python 2.5c1: 62121 (fix wrong lnotab with for loops and
    //                          storing constants that should have been removed)
    //     Python 2.5c2: 62131 (fix wrong code: for x, in ... in listcomp/genexp)
    //     Python 2.6a0: 62151 (peephole optimizations and STORE_MAP opcode)
    //     Python 2.6a1: 62161 (WITH_CLEANUP optimization)
    //     Python 2.7a0: 62171 (optimize list comprehensions/change LIST_APPEND)
    //     Python 2.7a0: 62181 (optimize conditional branches:
    //                          introduce POP_JUMP_IF_FALSE and POP_JUMP_IF_TRUE)
    //     Python 2.7a0  62191 (introduce SETUP_WITH)
    //     Python 2.7a0  62201 (introduce BUILD_SET)
    //     Python 2.7a0  62211 (introduce MAP_ADD and SET_ADD)
    //     Python 3000:   3000
    //                    3010 (removed UNARY_CONVERT)
    //                    3020 (added BUILD_SET)
    //                    3030 (added keyword-only parameters)
    //                    3040 (added signature annotations)
    //                    3050 (print becomes a function)
    //                    3060 (PEP 3115 metaclass syntax)
    //                    3061 (string literals become unicode)
    //                    3071 (PEP 3109 raise changes)
    //                    3081 (PEP 3137 make __file__ and __name__ unicode)
    //                    3091 (kill str8 interning)
    //                    3101 (merge from 2.6a0, see 62151)
    //                    3103 (__file__ points to source file)
    //     Python 3.0a4: 3111 (WITH_CLEANUP optimization).
    //     Python 3.0b1: 3131 (lexical exception stacking, including POP_EXCEPT
    //                         #3021)
    //     Python 3.1a1: 3141 (optimize list, set and dict comprehensions:
    //                         change LIST_APPEND and SET_ADD, add MAP_ADD //2183)
    //     Python 3.1a1: 3151 (optimize conditional branches:
    //                         introduce POP_JUMP_IF_FALSE and POP_JUMP_IF_TRUE
    //                         #4715)
    //     Python 3.2a1: 3160 (add SETUP_WITH //6101)
    //                   tag: cpython-32
    //     Python 3.2a2: 3170 (add DUP_TOP_TWO, remove DUP_TOPX and ROT_FOUR //9225)
    //                   tag: cpython-32
    //     Python 3.2a3  3180 (add DELETE_DEREF //4617)
    //     Python 3.3a1  3190 (__class__ super closure changed)
    //     Python 3.3a1  3200 (PEP 3155 __qualname__ added //13448)
    //     Python 3.3a1  3210 (added size modulo 2**32 to the pyc header //13645)
    //     Python 3.3a2  3220 (changed PEP 380 implementation //14230)
    //     Python 3.3a4  3230 (revert changes to implicit __class__ closure //14857)
    //     Python 3.4a1  3250 (evaluate positional default arguments before
    //                        keyword-only defaults //16967)
    //     Python 3.4a1  3260 (add LOAD_CLASSDEREF; allow locals of class to override
    //                        free vars //17853)
    //     Python 3.4a1  3270 (various tweaks to the __class__ closure //12370)
    //     Python 3.4a1  3280 (remove implicit class argument)
    //     Python 3.4a4  3290 (changes to __qualname__ computation //19301)
    //     Python 3.4a4  3300 (more changes to __qualname__ computation //19301)
    //     Python 3.4rc2 3310 (alter __qualname__ computation //20625)
    //     Python 3.5a1  3320 (PEP 465: Matrix multiplication operator //21176)
    //     Python 3.5b1  3330 (PEP 448: Additional Unpacking Generalizations //2292)
    //     Python 3.5b2  3340 (fix dictionary display evaluation order //11205)
    //     Python 3.5b3  3350 (add GET_YIELD_FROM_ITER opcode //24400)
    //     Python 3.5.2  3351 (fix BUILD_MAP_UNPACK_WITH_CALL opcode //27286)
    //     Python 3.6a0  3360 (add FORMAT_VALUE opcode //25483)
    //     Python 3.6a1  3361 (lineno delta of code.co_lnotab becomes signed //26107)
    //     Python 3.6a2  3370 (16 bit wordcode //26647)
    //     Python 3.6a2  3371 (add BUILD_CONST_KEY_MAP opcode //27140)
    //     Python 3.6a2  3372 (MAKE_FUNCTION simplification, remove MAKE_CLOSURE
    //                         #27095)
    //     Python 3.6b1  3373 (add BUILD_STRING opcode //27078)
    //     Python 3.6b1  3375 (add SETUP_ANNOTATIONS and STORE_ANNOTATION opcodes
    //                         #27985)
    //     Python 3.6b1  3376 (simplify CALL_FUNCTIONs & BUILD_MAP_UNPACK_WITH_CALL
    //                         #27213)
    //     Python 3.6b1  3377 (set __class__ cell from type.__new__ //23722)
    //     Python 3.6b2  3378 (add BUILD_TUPLE_UNPACK_WITH_CALL //28257)
    //     Python 3.6rc1 3379 (more thorough __class__ validation //23722)
    //     Python 3.7a1  3390 (add LOAD_METHOD and CALL_METHOD opcodes //26110)
    //     Python 3.7a2  3391 (update GET_AITER //31709)
    //     Python 3.7a4  3392 (PEP 552: Deterministic pycs //31650)
    //     Python 3.7b1  3393 (remove STORE_ANNOTATION opcode //32550)
    //     Python 3.7b5  3394 (restored docstring as the first stmt in the body;
    //                         this might affected the first line number //32911)
    //     Python 3.8a1  3400 (move frame block handling to compiler //17611)
    //     Python 3.8a1  3401 (add END_ASYNC_FOR //33041)
    //     Python 3.8a1  3410 (PEP570 Python Positional-Only Parameters //36540)
    //     Python 3.8b2  3411 (Reverse evaluation order of key: value in dict
    //                         comprehensions //35224)
    //     Python 3.8b2  3412 (Swap the position of positional args and positional
    //                         only args in ast.arguments //37593)
    //     Python 3.8b4  3413 (Fix "break" and "continue" in "finally" //37830)
    //     Python 3.9a0  3420 (add LOAD_ASSERTION_ERROR //34880)
    //     Python 3.9a0  3421 (simplified bytecode for with blocks //32949)
    //     Python 3.9a0  3422 (remove BEGIN_FINALLY, END_FINALLY, CALL_FINALLY, POP_FINALLY bytecodes //33387)
    //     Python 3.9a2  3423 (add IS_OP, CONTAINS_OP and JUMP_IF_NOT_EXC_MATCH bytecodes //39156)
    //     Python 3.9a2  3424 (simplify bytecodes for *value unpacking)
    //     Python 3.9a2  3425 (simplify bytecodes for **value unpacking)
    //     Python 3.10a1 3430 (Make 'annotations' future by default)
    //     Python 3.10a1 3431 (New line number table format -- PEP 626)
    //     Python 3.10a2 3432 (Function annotation for MAKE_FUNCTION is changed from dict to tuple bpo-42202)
    //     Python 3.10a2 3433 (RERAISE restores f_lasti if oparg != 0)
    //     Python 3.10a6 3434 (PEP 634: Structural Pattern Matching)
    //     Python 3.10a7 3435 Use instruction offsets (as opposed to byte offsets).
    //     Python 3.10b1 3436 (Add GEN_START bytecode //43683)
    //     Python 3.10b1 3437 (Undo making 'annotations' future by default - We like to dance among core devs!)
    //     Python 3.10b1 3438 Safer line number table handling.
    //     Python 3.10b1 3439 (Add ROT_N)
    //     Python 3.11a1 3450 Use exception table for unwinding ("zero cost" exception handling)
    //     Python 3.11a1 3451 (Add CALL_METHOD_KW)
    //     Python 3.11a1 3452 (drop nlocals from marshaled code objects)
    //     Python 3.11a1 3453 (add co_fastlocalnames and co_fastlocalkinds)
    //     Python 3.11a1 3454 (compute cell offsets relative to locals bpo-43693)
    //     Python 3.11a1 3455 (add MAKE_CELL bpo-43693)
    //     Python 3.11a1 3456 (interleave cell args bpo-43693)
    //     Python 3.11a1 3457 (Change localsplus to a bytes object bpo-43693)
    //     Python 3.11a1 3458 (imported objects now don't use LOAD_METHOD/CALL_METHOD)
    //     Python 3.11a1 3459 (PEP 657: add end line numbers and column offsets for instructions)
    //     Python 3.11a1 3460 (Add co_qualname field to PyCodeObject bpo-44530)
    //     Python 3.11a1 3461 (JUMP_ABSOLUTE must jump backwards)
    //     Python 3.11a2 3462 (bpo-44511: remove COPY_DICT_WITHOUT_KEYS, change
    //                         MATCH_CLASS and MATCH_KEYS, and add COPY)
    //     Python 3.11a3 3463 (bpo-45711: JUMP_IF_NOT_EXC_MATCH no longer pops the
    //                         active exception)
    //     Python 3.11a3 3464 (bpo-45636: Merge numeric BINARY_*/INPLACE_* into
    //                         BINARY_OP)
    //     Python 3.11a3 3465 (Add COPY_FREE_VARS opcode)
    //     Python 3.11a4 3466 (bpo-45292: PEP-654 except*)
    //     Python 3.11a4 3467 (Change CALL_xxx opcodes)
    //     Python 3.11a4 3468 (Add SEND opcode)
    //     Python 3.11a4 3469 (bpo-45711: remove type, traceback from exc_info)
    //     Python 3.11a4 3470 (bpo-46221: PREP_RERAISE_STAR no longer pushes lasti)
    //     Python 3.11a4 3471 (bpo-46202: remove pop POP_EXCEPT_AND_RERAISE)
    //     Python 3.11a4 3472 (bpo-46009: replace GEN_START with POP_TOP)
    //     Python 3.11a4 3473 (Add POP_JUMP_IF_NOT_NONE/POP_JUMP_IF_NONE opcodes)
    //     Python 3.11a4 3474 (Add RESUME opcode)
    //     Python 3.11a5 3475 (Add RETURN_GENERATOR opcode)
    //     Python 3.11a5 3476 (Add ASYNC_GEN_WRAP opcode)
    //     Python 3.11a5 3477 (Replace DUP_TOP/DUP_TOP_TWO with COPY and
    //                         ROT_TWO/ROT_THREE/ROT_FOUR/ROT_N with SWAP)
    //     Python 3.11a5 3478 (New CALL opcodes)
    //     Python 3.11a5 3479 (Add PUSH_NULL opcode)
    //     Python 3.11a5 3480 (New CALL opcodes, second iteration)
    //     Python 3.11a5 3481 (Use inline cache for BINARY_OP)
    //     Python 3.11a5 3482 (Use inline caching for UNPACK_SEQUENCE and LOAD_GLOBAL)
    //     Python 3.11a5 3483 (Use inline caching for COMPARE_OP and BINARY_SUBSCR)
    //     Python 3.11a5 3484 (Use inline caching for LOAD_ATTR, LOAD_METHOD, and
    //                         STORE_ATTR)
    //     Python 3.11a5 3485 (Add an oparg to GET_AWAITABLE)
    //     Python 3.11a6 3486 (Use inline caching for PRECALL and CALL)
    //     Python 3.11a6 3487 (Remove the adaptive "oparg counter" mechanism)
    //     Python 3.11a6 3488 (LOAD_GLOBAL can push additional NULL)
    //     Python 3.11a6 3489 (Add JUMP_BACKWARD, remove JUMP_ABSOLUTE)
    //     Python 3.11a6 3490 (remove JUMP_IF_NOT_EXC_MATCH, add CHECK_EXC_MATCH)
    //     Python 3.11a6 3491 (remove JUMP_IF_NOT_EG_MATCH, add CHECK_EG_MATCH,
    //                         add JUMP_BACKWARD_NO_INTERRUPT, make JUMP_NO_INTERRUPT virtual)
    //     Python 3.11a7 3492 (make POP_JUMP_IF_NONE/NOT_NONE/TRUE/FALSE relative)
    //     Python 3.11a7 3493 (Make JUMP_IF_TRUE_OR_POP/JUMP_IF_FALSE_OR_POP relative)
    //     Python 3.11a7 3494 (New location info table)
    //     Python 3.11b4 3495 (Set line number of module's RESUME instr to 0 per PEP 626)
    //     Python 3.12a1 3500 (Remove PRECALL opcode)
    //     Python 3.12a1 3501 (YIELD_VALUE oparg == stack_depth)
    //     Python 3.12a1 3502 (LOAD_FAST_CHECK, no NULL-check in LOAD_FAST)
    //     Python 3.12a1 3503 (Shrink LOAD_METHOD cache)
    //     Python 3.12a1 3504 (Merge LOAD_METHOD back into LOAD_ATTR)
    //     Python 3.12a1 3505 (Specialization/Cache for FOR_ITER)
    //     Python 3.12a1 3506 (Add BINARY_SLICE and STORE_SLICE instructions)
    //     Python 3.12a1 3507 (Set lineno of module's RESUME to 0)
    //     Python 3.12a1 3508 (Add CLEANUP_THROW)
    //     Python 3.12a1 3509 (Conditional jumps only jump forward)
    //     Python 3.12a2 3510 (FOR_ITER leaves iterator on the stack)
    //     Python 3.12a2 3511 (Add STOPITERATION_ERROR instruction)
    //     Python 3.12a2 3512 (Remove all unused consts from code objects)
    //     Python 3.12a4 3513 (Add CALL_INTRINSIC_1 instruction, removed STOPITERATION_ERROR, PRINT_EXPR, IMPORT_STAR)
    //     Python 3.12a4 3514 (Remove ASYNC_GEN_WRAP, LIST_TO_TUPLE, and UNARY_POSITIVE)
    //     Python 3.12a5 3515 (Embed jump mask in COMPARE_OP oparg)
    //     Python 3.12a5 3516 (Add COMPARE_AND_BRANCH instruction)
    //     Python 3.12a5 3517 (Change YIELD_VALUE oparg to exception block depth)
    //     Python 3.12a6 3518 (Add RETURN_CONST instruction)
    //     Python 3.12a6 3519 (Modify SEND instruction)
    //     Python 3.12a6 3520 (Remove PREP_RERAISE_STAR, add CALL_INTRINSIC_2)
    //     Python 3.12a7 3521 (Shrink the LOAD_GLOBAL caches)
    //     Python 3.12a7 3522 (Removed JUMP_IF_FALSE_OR_POP/JUMP_IF_TRUE_OR_POP)
    //     Python 3.12a7 3523 (Convert COMPARE_AND_BRANCH back to COMPARE_OP)
    //     Python 3.12a7 3524 (Shrink the BINARY_SUBSCR caches)
    //     Python 3.12b1 3525 (Shrink the CALL caches)
    //     Python 3.12b1 3526 (Add instrumentation support)
    //     Python 3.12b1 3527 (Add LOAD_SUPER_ATTR)
    //     Python 3.12b1 3528 (Add LOAD_SUPER_ATTR_METHOD specialization)
    //     Python 3.12b1 3529 (Inline list/dict/set comprehensions)
    //     Python 3.12b1 3530 (Shrink the LOAD_SUPER_ATTR caches)
    //     Python 3.12b1 3531 (Add PEP 695 changes)
    //     Python 3.13a1 3550 (Plugin optimizer support)
    //     Python 3.13a1 3551 (Compact superinstructions)
    //     Python 3.13a1 3552 (Remove LOAD_FAST__LOAD_CONST and LOAD_CONST__LOAD_FAST)
    //     Python 3.13a1 3553 (Add SET_FUNCTION_ATTRIBUTE)
    //     Python 3.13a1 3554 (more efficient bytecodes for f-strings)
    //     Python 3.13a1 3555 (generate specialized opcodes metadata from bytecodes.c)
    //     Python 3.13a1 3556 (Convert LOAD_CLOSURE to a pseudo-op)
    //     Python 3.13a1 3557 (Make the conversion to boolean in jumps explicit)
    //     Python 3.13a1 3558 (Reorder the stack items for CALL)
    //     Python 3.13a1 3559 (Generate opcode IDs from bytecodes.c)
    //     Python 3.13a1 3560 (Add RESUME_CHECK instruction)
    //     Python 3.13a1 3561 (Add cache entry to branch instructions)
    //     Python 3.13a1 3562 (Assign opcode IDs for internal ops in separate range)
    //     Python 3.13a1 3563 (Add CALL_KW and remove KW_NAMES)
    //     Python 3.13a1 3564 (Removed oparg from YIELD_VALUE, changed oparg values of RESUME)
    //     Python 3.13a1 3565 (Oparg of YIELD_VALUE indicates whether it is in a yield-from)
    //     Python 3.13a1 3566 (Emit JUMP_NO_INTERRUPT instead of JUMP for non-loop no-lineno cases)
    //     Python 3.13a1 3567 (Reimplement line number propagation by the compiler)
    //     Python 3.13a1 3568 (Change semantics of END_FOR)
    //     Python 3.13a5 3569 (Specialize CONTAINS_OP)
    //     Python 3.13a6 3570 (Add __firstlineno__ class attribute)
    //     Python 3.14a1 3600 (Add LOAD_COMMON_CONSTANT)
    //     Python 3.14a1 3601 (Fix miscompilation of private names in generic classes)
    //     Python 3.14a1 3602 (Add LOAD_SPECIAL. Remove BEFORE_WITH and BEFORE_ASYNC_WITH)
    //     Python 3.14a1 3603 (Remove BUILD_CONST_KEY_MAP)
    //
    //     Python 3.15 will start with 3650

    if &buf[2..] != PYC_MAGIC {
        return Err(super::Error::BadMagic(2, buf[2..].to_vec(), PYC_MAGIC).into());
    }

    let val = ((buf[1] as u32) << 8) + (buf[0] as u32);

    #[allow(overlapping_range_endpoints)]
    #[allow(clippy::match_overlapping_arm)]
    match val {
        20121 => Ok(((1, 5), 8)),
        50428 => Ok(((1, 6), 8)),
        50823 => Ok(((2, 0), 8)),
        60202 => Ok(((2, 1), 8)),
        60717 => Ok(((2, 2), 8)),
        62011 | 62021 => Ok(((2, 3), 8)),
        62041 | 62051 | 62061 => Ok(((2, 4), 8)),
        62071 | 62081 | 62091 | 62092 | 62101 | 62111 | 62121 | 62131 => Ok(((2, 5), 8)),
        62151 | 62161 => Ok(((2, 6), 8)),
        62171 | 62181 | 62191 | 62201 | 62211 => Ok(((2, 7), 8)),
        3000..=3131 => Ok(((3, 0), 8)),
        3000..=3151 => Ok(((3, 1), 8)),
        3000..=3160 => Ok(((3, 1), 8)),
        3000..=3180 => Ok(((3, 2), 8)),
        3000..=3230 => Ok(((3, 3), 12)),
        3000..=3310 => Ok(((3, 4), 12)),
        3000..=3351 => Ok(((3, 5), 12)),
        3360 | 3361 | 3370..=3379 => Ok(((3, 6), 12)),
        3390..=3394 => Ok(((3, 7), 16)),
        3400 | 3401 | 3410..=3413 => Ok(((3, 8), 16)),
        3400..=3425 => Ok(((3, 9), 16)),
        3430..=3439 => Ok(((3, 10), 16)),
        3450..=3495 => Ok(((3, 11), 16)),
        3500..=3531 => Ok(((3, 12), 16)),
        3550..=3599 => Ok(((3, 13), 16)),
        3600..=3649 => Ok(((3, 14), 16)),
        3650..=4000 => Ok(((3, 15), 16)),
        _ => Err(super::Error::Other(
            format!("not a pyc file, unknown version magic {}", val)
        ).into()),
    }
}

fn format_flag(show_flag: bool, flag_num: &Option<usize>) -> Option<String> {
    if show_flag && flag_num.is_some() {
        Some(format!(" 🚩{}", flag_num.unwrap()))
    } else {
        None
    }
}

#[derive(Debug, Eq)]
struct CodeObject {
    argcount: u32,
    posonlyargcount: Option<u32>,
    kwonlyargcount: u32,
    nlocals: Option<u32>,
    stacksize: u32,
    flags: u32,
    code: Rc<Object>,
    consts: Rc<Object>,
    names: Rc<Object>,
    varnames: Option<Rc<Object>>,
    freevars: Option<Rc<Object>>,
    cellvars: Option<Rc<Object>>,
    localsplusnames: Option<Rc<Object>>,
    localspluskinds: Option<Rc<Object>>,
    filename: Rc<Object>,
    name: Rc<Object>,
    qualname: Option<Rc<Object>>,
    firstlineno: u32,
    linetable: Rc<Object>,
    exceptiontable: Option<Rc<Object>>,

    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for CodeObject {
    fn eq(&self, other: &Self) -> bool {
        self.argcount == other.argcount &&
            self.posonlyargcount == other.posonlyargcount &&
            self.kwonlyargcount == other.kwonlyargcount &&
            self.nlocals == other.nlocals &&
            self.stacksize == other.stacksize &&
            self.flags == other.flags &&
            self.code == other.code &&
            self.consts == other.consts &&
            self.names == other.names &&
            self.varnames == other.varnames &&
            self.freevars == other.freevars &&
            self.cellvars == other.cellvars &&
            self.localsplusnames == other.localsplusnames &&
            self.localspluskinds == other.localspluskinds &&
            self.filename == other.filename &&
            self.name == other.name &&
            self.qualname == other.qualname &&
            self.firstlineno == other.firstlineno &&
            self.linetable == other.linetable &&
            self.exceptiontable == other.exceptiontable
    }
}

impl Hash for CodeObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.argcount.hash(state);
        self.posonlyargcount.hash(state);
        self.kwonlyargcount.hash(state);
        self.nlocals.hash(state);
        self.stacksize.hash(state);
        self.flags.hash(state);
        self.code.hash(state);
        self.consts.hash(state);
        self.names.hash(state);
        self.varnames.hash(state);
        self.freevars.hash(state);
        self.cellvars.hash(state);
        self.localsplusnames.hash(state);
        self.localspluskinds.hash(state);
        self.filename.hash(state);
        self.name.hash(state);
        self.qualname.hash(state);
        self.firstlineno.hash(state);
        self.linetable.hash(state);
        self.exceptiontable.hash(state);
    }
}

impl CodeObject {
    fn pretty_print_binary_string<W>(
        w: &mut W,
        indent: &str,
        name: &str,
        mut object: &Rc<Object>,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        let (ref_info, show_target_flag);
        if let Object::Ref(v) = object.as_ref() {
            ref_info = format!(
                "(ref to {}){}",
                v.number,
                format_flag(show_flag, &v.flag_num).unwrap_or("".to_string()),
            );
            show_target_flag = false; // suppress printing of flag after we show ref info
            object = &v.target;
        } else {
            ref_info = "".to_string();
            show_target_flag = true;
        };

        if let Object::String(v) = object.as_ref() {
            if !v.bytes.is_empty() {
                return write!(w, "\n{indent}-{name}: {}[{} bytes]", ref_info, v.bytes.len())
            }
        }
        object.pretty_print(w, &format!("\n{indent}-{name}: {}", ref_info), "", true, show_target_flag)
    }

    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        multiline: bool,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        write!(w, "{prefix}Code")?;
        self.name.pretty_print(w, " ", "", false, true)?;
        if let Some(v) = &self.qualname {
            v.pretty_print(w, "/", "", false, true)?;
        }

        if let Some(s) = format_flag(show_flag, &self.flag_num) {
            write!(w, "{}", s)?;
        }

        if multiline {
            let indent = " ".repeat(prefix.len() + 2);

            self.filename.pretty_print(w, &format!("\n{indent}"), "", true, true)?;
            write!(w, ":{}", self.firstlineno)?;

            write!(w, "\n{indent}argcount={}", self.argcount)?;
            if let Some(v) = self.posonlyargcount {
                write!(w, " posonlyargcount={}", v)?;
            }
            write!(w, " kwonlyargcount={}", self.kwonlyargcount)?;
            if let Some(v) = self.nlocals {
                write!(w, " nlocals={}", v)?;
            }
            write!(w, " stacksize={}", self.stacksize)?;
            write!(w, " flags={:x}", self.flags)?;

            // We expect StringVariant::String with bytecode here.
            // Let's not print that out, since it's not going to be
            // readable in any way. Otherwise, just print the object.
            Self::pretty_print_binary_string(w, &indent, "code", &self.code, true)?;

            self.consts.pretty_print(w, &format!("\n{indent}-consts: "), "", true, true)?;
            self.names.pretty_print(w, &format!("\n{indent}-names: "), "", true, true)?;
            if let Some(v) = &self.varnames {
                v.pretty_print(w, &format!("\n{indent}-varnames: "), "", true, true)?;
            }
            if let Some(v) = &self.freevars {
                v.pretty_print(w, &format!("\n{indent}-freevars: "), "", true, true)?;
            }
            if let Some(v) = &self.cellvars {
                v.pretty_print(w, &format!("\n{indent}-cellvars: "), "", true, true)?;
            }
            if let Some(v) = &self.localsplusnames {
                v.pretty_print(w, &format!("\n{indent}-locals+names: "), "", true, true)?;
            }
            if let Some(v) = &self.localspluskinds {
                v.pretty_print(w, &format!("\n{indent}-locals+kinds: "), "", true, true)?;
            }
            Self::pretty_print_binary_string(w, &indent, "linetable", &self.linetable, true)?;
            if let Some(v) = &self.exceptiontable {
                Self::pretty_print_binary_string(w, &indent, "exceptiontable", v, true)?;
            }
        }

        write!(w, "{suffix}")
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
enum StringVariant {
    ShortAscii,
    ShortAsciiInterned,
    String,
    Interned,
    Unicode,
    Ascii,
    AsciiInterned,
}

#[derive(Debug, Eq)]
struct StringObject {
    variant: StringVariant,
    bytes: Vec<u8>,

    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for StringObject {
    fn eq(&self, other: &Self) -> bool {
        self.variant == other.variant &&
            self.bytes == other.bytes
    }
}

impl Hash for StringObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.variant.hash(state);
        self.bytes.hash(state);
    }
}

impl fmt::Display for StringObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.variant {
            StringVariant::ShortAscii |
            StringVariant::ShortAsciiInterned |
            StringVariant::Unicode |
            StringVariant::Ascii |
            StringVariant::AsciiInterned => {
                if let Ok(string) = str::from_utf8(&self.bytes) {
                    write!(f, "{:?}", string)
                } else {
                    write!(f, "[NON-UTF8] {:?}", self.bytes)
                }
            }
            StringVariant::String |
            StringVariant::Interned => {
                write!(f, "{:?}", self.bytes)
            }
        }
    }
}

impl StringObject {
    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        _multiline: bool,
        show_flag: bool,
) -> fmt::Result
    where
        W: fmt::Write,
    {
        write!(
            w, "{prefix}{}{}{suffix}",
            self,
            format_flag(show_flag, &self.flag_num).unwrap_or("".to_string()),
        )
    }
}

#[derive(Debug, Eq, PartialEq, Hash)]
enum SeqVariant {
    Tuple,
    List,
    Set,
    FrozenSet,
}

#[derive(Debug, Eq)]
struct SeqObject {
    variant: SeqVariant,
    items: Vec<Rc<Object>>,

    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for SeqObject {
    fn eq(&self, other: &Self) -> bool {
        self.variant == other.variant &&
            self.items == other.items
    }
}

impl Hash for SeqObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.variant.hash(state);
        self.items.hash(state);
    }
}

impl SeqObject {
    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        multiline: bool,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        let (beg, end);
        let mut extra_comma = "";
        let multiline = multiline && self.need_multiline(1);
        let indent = prefix
            .chars()
            .skip_while(|ch| *ch == '\n')
            .take_while(|ch| ch.is_whitespace())
            .count();

        match self.variant {
            SeqVariant::Tuple => {
                beg = "(";
                end = ")";
                if self.items.len() == 1 {
                    extra_comma = ",";
                }
            }
            SeqVariant::List => {
                beg = "[";
                end = "]";
            }
            SeqVariant::Set => {
                if self.items.is_empty() {
                    beg = "set(";
                    end = ")";
                } else {
                    beg = "{";
                    end = "}";
                }
            }
            SeqVariant::FrozenSet => {
                if self.items.is_empty() {
                    beg = "frozenset(";
                    end = ")";
                } else {
                    beg = "frozenset({";
                    end = "})";
                }
            }
        }

        write!(w, "{prefix}{beg}")?;
        for (n, v) in self.items.iter().enumerate() {
            if multiline {
                if n == 0 {
                    writeln!(w)?;
                }
                v.pretty_print(
                    w,
                    &" ".repeat(indent + 2),
                    ",\n",
                    true,
                    true,
                )?;
            } else {
                v.pretty_print(
                    w,
                    if n > 0 { ", " } else { "" },
                    extra_comma,
                    false,
                    true,
                )?;
            }
        }

        write!(
            w, "{:>width$}{end}{}{suffix}",
            "",
            format_flag(show_flag, &self.flag_num).unwrap_or("".to_string()),
            width=multiline as usize * indent,
        )
    }

    fn need_multiline(&self, max_nesting: u8) -> bool {
        max_nesting == 0 ||
            self.items.len() > 10 ||
            self.items.iter().any(|x| x.need_multiline(max_nesting - 1))
    }
}

#[derive(Debug, Eq)]
struct DictObject {
    items: Vec<(Rc<Object>, Rc<Object>)>,

    #[allow(dead_code)]
    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for DictObject {
    fn eq(&self, other: &Self) -> bool {
        self.items == other.items
    }
}

impl Hash for DictObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.items.hash(state);
    }
}

impl DictObject {
    fn need_multiline(&self, max_nesting: u8) -> bool {
        max_nesting == 0 ||
            self.items.iter().any(|(x, y)| x.need_multiline(max_nesting - 1) || y.need_multiline(max_nesting - 1))
    }
}

#[derive(Debug, Eq)]
struct SliceObject {
    start: Rc<Object>,
    stop: Rc<Object>,
    step: Rc<Object>,

    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for SliceObject {
    fn eq(&self, other: &Self) -> bool {
        self.start == other.start &&
            self.stop == other.stop &&
            self.step == other.step
    }
}

impl Hash for SliceObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.start.hash(state);
        self.stop.hash(state);
        self.step.hash(state);
    }
}

impl SliceObject {
    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        _multiline: bool,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        self.start.pretty_print(w, &format!("{} slice(", prefix), "", false, true)?;
        self.stop.pretty_print(w, ", ", "", false, true)?;
        self.step.pretty_print(w, ", ", "", false, true)?;
        write!(
            w, "){}{suffix}",
            format_flag(show_flag, &self.flag_num).unwrap_or("".to_string()),
        )
    }
}

#[derive(Debug, Eq)]
struct RefObject {
    number: u64,

    target: Rc<Object>,
    flag_num: Option<usize>, // filled in when the object was stored with a flag_ref
}

impl PartialEq for RefObject {
    // We really care whether the target is the same.
    // When writing, we'll dereference the Ref to get to the contents.

    fn eq(&self, other: &Self) -> bool {
        self.target == other.target
    }
}

impl Hash for RefObject {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.target.hash(state);
    }
}

impl RefObject {
    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        multiline: bool,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        let prefix = format!("{prefix}(ref to {}){}",
                             self.number,
                             format_flag(show_flag, &self.flag_num).unwrap_or("".to_string()));
        self.target.pretty_print(w, &prefix, suffix, multiline, false)
    }
}

#[derive(Debug, Eq)]
enum Object {
    Code(CodeObject),
    Long(BigInt, Option<usize>),
    Int(u32, Option<usize>),
    String(StringObject),
    Seq(SeqObject),
    Null(Option<usize>),
    None(Option<usize>),
    True(Option<usize>),
    False(Option<usize>),
    StopIteration(Option<usize>),
    Ellipsis(Option<usize>),
    Float(u64, Option<usize>),        // yes, u64, so Rust allows Eq to be implemented
    Complex(u64, u64, Option<usize>),
    Dict(DictObject),
    Slice(SliceObject),
    Ref(RefObject),
}

impl PartialEq for Object {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            // For References, we want to actually look at the
            // reference target, so dereference before comparing.
            (Object::Ref(v), w) => v.target.deref().eq(w),
            (v, Object::Ref(w)) => v.eq(w.target.deref()),

            (Object::Code(v), Object::Code(w)) => v == w,
            (Object::Long(v, _), Object::Long(w, _)) => v == w,
            (Object::Int(v, _), Object::Int(w, _)) => v == w,
            (Object::Null(_), Object::Null(_)) => true,
            (Object::None(_), Object::None(_)) => true,
            (Object::True(_), Object::True(_)) => true,
            (Object::False(_),Object::False(_)) => true,
            (Object::StopIteration(_), Object::StopIteration(_)) => true,
            (Object::Ellipsis(_), Object::Ellipsis(_)) => true,
            (Object::Float(v, _), Object::Float(w, _)) => v == w,
            (Object::Complex(x, y, _), Object::Complex(u, v, _)) => x == u && y == v,
            (Object::String(v), Object::String(w)) => v == w,
            (Object::Seq(v), Object::Seq(w)) => v == w,
            (Object::Dict(v), Object::Dict(w)) => v == w,
            (Object::Slice(v), Object::Slice(w)) => v == w,
            _ => false,
        }
    }
}

impl Hash for Object {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            Object::Code(v) => v.hash(state),
            Object::String(v) => v.hash(state),
            Object::Seq(v) => v.hash(state),
            Object::Ref(v) => v.hash(state),
            Object::Dict(v) => v.hash(state),
            Object::Slice(v) => v.hash(state),

            Object::Long(v, _) => v.hash(state),
            Object::Int(v, _) => v.hash(state),
            Object::Null(_) => b'0'.hash(state),
            Object::None(_) => b'N'.hash(state),
            Object::True(_) => b'T'.hash(state),
            Object::False(_) => b'F'.hash(state),
            Object::StopIteration(_) => b'S'.hash(state),
            Object::Ellipsis(_) => b'.'.hash(state),
            Object::Float(v, _) => v.hash(state),
            Object::Complex(x, y, _) => {
                x.hash(state);
                y.hash(state);
            }
        }
    }
}

impl Object {
    #[allow(clippy::write_literal)]
    pub fn pretty_print<W>(
        &self,
        w: &mut W,
        prefix: &str,
        suffix: &str,
        multiline: bool,
        show_flag: bool,
    ) -> fmt::Result
    where
        W: fmt::Write,
    {
        let (s, flag_num) = match self {
            Object::Code(v) => {
                return v.pretty_print(w, prefix, suffix, multiline, show_flag);
            }
            Object::String(v) => {
                return v.pretty_print(w, prefix, suffix, multiline, show_flag);
            }
            Object::Seq(v) => {
                return v.pretty_print(w, prefix, suffix, multiline, show_flag);
            }
            Object::Slice(v) => {
                return v.pretty_print(w, prefix, suffix, multiline, show_flag);
            }
            Object::Ref(v) => {
                return v.pretty_print(w, prefix, suffix, multiline, show_flag);
            }
            Object::Dict(_) => todo!(),

            Object::Long(v, flag_num) => (format!("{v}"), flag_num),
            Object::Int(v, flag_num) => (format!("{v}"), flag_num),
            Object::Null(flag_num) => ("NULL".to_string(), flag_num),
            Object::None(flag_num) => ("None".to_string(), flag_num),
            Object::True(flag_num) => ("True".to_string(), flag_num),
            Object::False(flag_num) => ("False".to_string(), flag_num),
            Object::StopIteration(flag_num) => ("StopIteration".to_string(), flag_num),
            Object::Ellipsis(flag_num) => ("...".to_string(), flag_num),
            Object::Float(v, flag_num) => (format!("{v}"), flag_num),
            Object::Complex(x, y, flag_num) => (format!("{x}+{y}j"), flag_num),
        };

        write!(
            w, "{prefix}{}{}{suffix}",
            s,
            format_flag(show_flag, flag_num).unwrap_or("".to_string())
        )
    }

    fn need_multiline(&self, max_nesting: u8) -> bool {
        match self {
            Object::Code(..) => true,
            Object::Ref(..) => false,
            Object::Slice(..) |
            Object::Long(..) |
            Object::Int(..) |
            Object::Null(..) |
            Object::None(..) |
            Object::True(..) |
            Object::False(..) |
            Object::StopIteration(..) |
            Object::Ellipsis(..) |
            Object::Float(..) |
            Object::Complex(..) |
            Object::String(..) => false,
            Object::Seq(v) => v.need_multiline(max_nesting),
            Object::Dict(v) => v.need_multiline(max_nesting),
        }
    }
}

pub struct PycParser {
    input_path: PathBuf,
    pub version: (u32, u32),
    header_length: usize,

    data: Vec<u8>,      // the whole contents of the input file
    read_offset: usize, // index into .data

    flag_refs: Vec<Option<Rc<Object>>>, // objects that have been flagged to be referenced
}

impl PycParser {
    pub fn from_file(input_path: &Path, mut input: impl io::Read) -> Result<Self> {
        let mut buf = [0; 4];
        input.read_exact(&mut buf)?;

        let (version, header_length) = pyc_python_version(&buf)?;
        debug!("{}: pyc file for Python {}.{}", input_path.display(), version.0, version.1);
        if TRACE {
            debug!("{}: pyc file header is {} bytes", input_path.display(), header_length);
        }

        let mut data = Vec::from(&buf);
        input.read_to_end(&mut data)?;

        if data.len() < header_length {
            return Err(super::Error::Other(
                format!("pyc file is too short ({} < {})", data.len(), header_length)
            ).into());
        }

        let pyc = PycParser {
            input_path: input_path.to_path_buf(),
            version,
            header_length,
            data,
            read_offset: header_length,
            flag_refs: Vec::new(),
        };

        let mtime = pyc.py_content_mtime();
        // 'size' seems to be the count of serialized objects, excluding TYPE_REF
        debug!("{}: from py with mtime={} ({}), size={} bytes, {}",
               input_path.display(),
               mtime,
               chrono::DateTime::from_timestamp(mtime as i64, 0).unwrap(),
               pyc.py_content_size(),
               match pyc.py_content_hash() {
                   None | Some(0) => "no hash invalidation".to_string(),
                   Some(hash) => format!("hash={hash}"),
               }
        );

        // TODO: check if .py file exists, and if yes, check if mtime
        // read above matches the mtime on the file, and if the size
        // read above matches the size of the source file. If not,
        // warn that something is awry and Python would rewrite the
        // bytecode. Consider adjusting the mtime and hash to match.

        Ok(pyc)
    }

    pub fn py_content_hash(&self) -> Option<u32> {
        if self.version < (3, 7) { // The first version supporting PEP 552
            None
        } else {
            match self._read_long_at(4) {
                0 => None,  // Let's always map 0 to None.
                v => Some(v),
            }
        }
    }

    pub fn py_content_mtime(&self) -> u32 {
        let offset = if self.version < (3, 7) { 4 } else { 8 };
        self._read_long_at(offset)
    }

    pub fn py_content_size(&self) -> u32 {
        let offset = if self.version < (3, 7) { 8 } else { 12 };
        self._read_long_at(offset)
    }

    fn take(&mut self, count: usize) -> Result<usize> {
        // This just checks availability and moves the offset.
        // The return value points to the beginning of data.

        if self.read_offset + count <= self.data.len() {
            let offset = self.read_offset;
            self.read_offset += count;
            Ok(offset)
        } else {
            Err(super::Error::UnexpectedEOF(self.read_offset as u64, count).into())
        }
    }

    fn _read_byte(&mut self) -> Result<(usize, u8)> {
        let offset = self.take(1)?;
        Ok((offset, self.data[offset]))
    }

    fn read_object(&mut self) -> Result<Rc<Object>> {
        let flag_num: Option<usize>;
        let (offset, mut b) = self._read_byte()?;

        if (b & FLAG_REF_BIT) != 0 {
            // This object has been flagged for future references. We
            // put it on the list of objects which can be referred to
            // later by index in the pyc stream.
            b &= !FLAG_REF_BIT;

            // We reserve the reference index number early.
            // We'll put the constructed object into the slot later.
            flag_num = Some(self.flag_refs.len());
            self.flag_refs.push(None);
        } else {
            flag_num = None;
        }

        if TRACE {
            debug!("{}:{}/0x{:x}: type {:?}{}",
                   self.input_path.display(), offset, offset,
                   b as char,
                   flag_num.map_or("".to_string(), |n| format!(" 🚩{}", n)),
            );
        }

        let obj = match b {
            b'0' => Object::Null(flag_num).into(),
            b'N' => Object::None(flag_num).into(),
            b'F' => Object::False(flag_num).into(),
            b'T' => Object::True(flag_num).into(),
            b'.' => Object::Ellipsis(flag_num).into(),
            b'S' => Object::StopIteration(flag_num).into(),

            b'c'    // CODE
                => self.read_codeobject(flag_num)?,
            b'g'    // BINARY_FLOAT
                => self.read_binary_float(flag_num)?,
            b'i'    // INT
                => self.read_long(flag_num)?,
            b'l'    // LONG
                => self.read_py_long(flag_num)?,
            b'y'    // BINARY_COMPLEX
                => self.read_binary_complex(flag_num)?,

            b'r'    // REF
                => self.read_ref(flag_num)?,

            b'z'    // SHORT_ASCII
                => self.read_string(StringVariant::ShortAscii, flag_num)?,
            b'Z'    // SHORT_ASCII_INTERNED
                => self.read_string(StringVariant::ShortAsciiInterned, flag_num)?,
            b's'    // STRING
                => self.read_string(StringVariant::String, flag_num)?,
            b't'    // INTERNED
                => self.read_string(StringVariant::Interned, flag_num)?,
            b'u'    // UNICODE
                => self.read_string(StringVariant::Unicode, flag_num)?,
            b'a'    // ASCII
                => self.read_string(StringVariant::Ascii, flag_num)?,
            b'A'    // ASCII_INTERNED
                => self.read_string(StringVariant::AsciiInterned, flag_num)?,
            b')'    // SMALL_TUPLE
                => self.read_small_tuple(flag_num)?,
            b'('    // TUPLE
                => self.read_seq(SeqVariant::Tuple, flag_num)?,
            b'['    // LIST
                => self.read_seq(SeqVariant::List, flag_num)?,
            b'<'    // SET
                => self.read_seq(SeqVariant::Set, flag_num)?,
            b'>'    // FROZEN_SET
                => self.read_seq(SeqVariant::FrozenSet, flag_num)?,
            b'{'    // DICT
                => self.read_dict(flag_num)?,
            b':'    // SLICE
                => self.read_slice(flag_num)?,

            b'I' |  // INT64
            b'f' |  // FLOAT
            b'x' |  // COMPLEX
            b'?'    // UNKNOWN
                => {
                    return Err(super::Error::Other(
                        format!("{}:{}/0x{:x}: unimplemented object type {}/'{}'",
                                self.input_path.display(), offset, offset,
                                b, b as char)
                    ).into());
                },
            _
                => {
                    return Err(super::Error::Other(
                        format!("{}:{}/0x{:x}: unknown object type {}/'{}'",
                                self.input_path.display(), offset, offset,
                                b, b as char)
                    ).into());
                },
        };

        if TRACE {
            dbg!(&obj);
        }

        if let Some(flag_num) = flag_num {
            assert!(self.flag_refs[flag_num].is_none());
            self.flag_refs[flag_num] = Some(obj.clone());
        }

        Ok(obj)
    }

    fn _maybe_read_long(&mut self, cond: bool) -> Result<Option<u32>> {
        Ok(if cond { Some(self._read_long()?) } else { None })
    }

    fn maybe_read_object(&mut self, cond: bool) -> Result<Option<Rc<Object>>> {
        Ok(if cond {
            Some(self.read_object()?)
        } else {
            None
        })
    }

    fn read_codeobject(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        Ok(Object::Code(CodeObject {
            argcount: self._read_long()?,
            posonlyargcount: self._maybe_read_long(self.version >= (3, 8))?,
            kwonlyargcount: self._read_long()?,
            nlocals: self._maybe_read_long(self.version < (3, 11))?,
            stacksize: self._read_long()?,
            flags: self._read_long()?,
            code: self.read_object()?,
            consts: self.read_object()?,
            names: self.read_object()?,
            varnames: self.maybe_read_object(self.version < (3, 11))?,
            freevars: self.maybe_read_object(self.version < (3, 11))?,
            cellvars: self.maybe_read_object(self.version < (3, 11))?,
            localsplusnames: self.maybe_read_object(self.version >= (3, 11))?,
            localspluskinds: self.maybe_read_object(self.version >= (3, 11))?,
            filename: self.read_object()?,
            name: self.read_object()?,
            qualname: self.maybe_read_object(self.version >= (3, 11))?,
            firstlineno: self._read_long()?,
            linetable: self.read_object()?,
            exceptiontable: self.maybe_read_object(self.version >= (3, 11))?,
            flag_num,
        }).into())
    }

    fn _read_long_at(&self, offset: usize) -> u32 {
        let bytes = &self.data[offset .. offset + 4];
        u32::from_le_bytes(bytes.try_into().unwrap())
    }

    fn _read_long(&mut self) -> Result<u32> {
        let offset = self.take(4)?;
        Ok(self._read_long_at(offset))
    }

    fn _read_long_signed(&mut self) -> Result<i32> {
        let offset = self.take(4)?;
        let bytes = &self.data[offset .. offset + 4];
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_long(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        Ok(Object::Int(self._read_long()?, flag_num).into())
    }

    fn _read_short(&mut self) -> Result<i32> {
        let offset = self.take(2)?;

        let x = (self.data[offset] as i32) + ((self.data[offset + 1] as i32) << 8);
        // Sign-extension, in case short greater than 16 bits
        Ok(x | -(x & 0x8000))
    }

    fn read_py_long(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let n = self._read_long_signed()?;

        let mut result = 0_i32.to_bigint().unwrap();
        for i in 0 .. n.abs() {
            let part = self._read_short()?;
            result += part.to_bigint().unwrap() << (i * PYLONG_MARSHAL_SHIFT) as usize;
        }

        Ok(Object::Long(result * n.signum(), flag_num).into())
    }

    fn read_string(&mut self, variant: StringVariant, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let size = match variant {
            // short == size is stored as one byte
            StringVariant::ShortAscii |
            StringVariant::ShortAsciiInterned
                => self._read_byte()?.1 as usize,
            // non-short == size is stored as long (4 bytes)
            StringVariant::String |
            StringVariant::Interned |
            StringVariant::Unicode |
            StringVariant::Ascii |
            StringVariant::AsciiInterned
                => self._read_long()? as usize,
        };

        let offset = self.take(size)?;
        Ok(Object::String(StringObject {
            variant,
            bytes: self.data[offset .. offset + size].to_vec(),
            flag_num,
        }).into())
    }

    fn _read_tuple(&mut self, variant: SeqVariant, size: u64, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let mut items = Vec::new();
        for _ in 0..size {
            items.push(self.read_object()?);
        }

        Ok(Object::Seq(SeqObject { variant, items, flag_num }).into())
    }

    fn read_small_tuple(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        // small tuple — size is only one byte
        let size = self._read_byte()?.1;
        self._read_tuple(SeqVariant::Tuple, size as u64, flag_num)
    }

    fn read_seq(&mut self, variant: SeqVariant, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let size = self._read_long()?;
        self._read_tuple(variant, size as u64, flag_num)
    }

    fn read_ref(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let index = self._read_long()?;

        // Is this a valid reference to one of the already-flagged objects?
        if index as usize >= self.flag_refs.len() {
            return Err(super::Error::Other(
                format!("{}:{}/0x{:x}: bad reference to flag_ref {} (have {})",
                        self.input_path.display(), self.read_offset, self.read_offset,
                        index, self.flag_refs.len())
            ).into());
        }

        let target = match &self.flag_refs[index as usize] {
            None => {
                return Err(super::Error::Other(
                    format!("{}:{}/0x{:x}: bad reference to flag_ref {} (reference from within)",
                            self.input_path.display(), self.read_offset, self.read_offset,
                            index)
                ).into());
            }
            Some(v) => v
        };

        Ok(Object::Ref(RefObject {
            number: index as u64,
            target: target.clone(),
            flag_num,
        }).into())
    }

    fn _read_binary_float(&mut self) -> Result<f64> {
        let offset = self.take(8)?;
        let bytes = &self.data[offset .. offset + 8];
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_binary_float(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        Ok(Object::Float(
            self._read_binary_float()?.to_bits(),
            flag_num,
        ).into())
    }

    fn read_binary_complex(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        Ok(Object::Complex(
            self._read_binary_float()?.to_bits(),
            self._read_binary_float()?.to_bits(),
            flag_num,
        ).into())
    }

    fn read_dict(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let mut items = Vec::new();

        loop {
            let key = self.read_object()?;
            if let Object::Null(..) = *key {
                break;
            }

            let value = self.read_object()?;
            items.push((key, value));
        }

        Ok(Object::Dict(DictObject { items, flag_num } ).into())
    }

    fn read_slice(&mut self, flag_num: Option<usize>) -> Result<Rc<Object>> {
        let start = self.read_object()?;
        let stop = self.read_object()?;
        let step = self.read_object()?;

        Ok(Object::Slice(SliceObject { start, stop, step, flag_num } ).into())
    }

    fn set_zero_mtime(&mut self) -> Result<bool> {
        // Set the embedded mtime timestamp of the source .py file to 0 in the header.

        if self.py_content_mtime() == 0 {
            return Ok(false);
        }

        let offset = if self.version < (3, 7) { 4 } else { 8 };
        self.data[offset..offset+4].fill(0);
        assert!(self.py_content_mtime() == 0);

        Ok(true)
    }
}

type SeenState = (usize, usize, RefCell<Option<usize>>);

struct PycWriter {
    buffer: Vec<u8>,
    seen: HashMap<Rc<Object>, SeenState>, // map object -> (offset, reference count, flag_num)
    flag_num: usize,
    refs_to_fix: HashMap<usize, Rc<Object>>, // map offsets to ref -> object
    entry_count: usize,
}

impl PycWriter {
    fn new(header: &[u8]) -> Self {
        Self {
            buffer: Vec::from(header),
            seen: HashMap::new(),
            flag_num: 0,
            refs_to_fix: HashMap::new(),
            entry_count: 0,
        }
    }

    fn to_buffer(parser: &PycParser, code: &Rc<Object>) -> Vec<u8> {
        // Copy the header from original file
        let mut w = PycWriter::new(
            &parser.data[..parser.header_length],
        );

        w.write_object(code);
        w.add_ref_flags();
        w.fix_refs();

        w.buffer
    }

    fn write_object(&mut self, object: &Rc<Object>) {
        if let Object::Ref(v) = &**object {
            self.write_object(&v.target);

        } else if self.seen.contains_key(object) {
            if TRACE {
                debug!("Referencing {:?} -> {:?}", object, self.seen[object]);
            }

            self.seen.entry(object.clone()).and_modify(|tup| tup.1 += 1);
            self.write_ref(object.clone());

        } else {
            let offset = self.buffer.len();
            self.entry_count += 1;

            match &**object {
                // Those end up in the index
                Object::Code(v) => {
                    self.write_code(v);
                },
                Object::String(v) => {
                    self.write_string(v);
                },
                Object::Seq(v) => {
                    self.write_seq(v);
                }
                Object::Slice(v) => {
                    self.write_slice(v);
                }
                Object::Dict(_) => todo!(),
                // mind null termination!

                Object::Long(v, _) => {
                    self.write_long(v);
                }
                Object::Int(v, _) => {
                    self.write_int(*v);
                }
                Object::Float(v, _) => {
                    self.write_binary_float(*v);
                }
                Object::Complex(x, y, _) => {
                    self.write_binary_complex(*x, *y);
                }

                // Those are not in the index. The reference takes as
                // many bytes or more to write as the object itself.
                Object::Ref(_) => {
                    panic!(); // already handled above.
                }
                Object::Null(_) => {
                    return self.buffer.push(b'0');
                }
                Object::None(_) => {
                    return self.buffer.push(b'N');
                }
                Object::False(_) =>  {
                    return self.buffer.push(b'F');
                }
                Object::True(_) =>  {
                    return self.buffer.push(b'T');
                }
                Object::StopIteration(_) =>  {
                    return self.buffer.push(b'S');
                }
                Object::Ellipsis(_) =>  {
                    return self.buffer.push(b'.');
                }
            }

            self.seen.insert(object.clone(), (offset, 0, None.into()));
        }
    }

    fn maybe_write_object(&mut self, object: &Option<Rc<Object>>) {
        if let Some(object) = object {
            self.write_object(object);
        }
    }

    fn write_code(&mut self, code: &CodeObject) {
        self.buffer.push(b'c');

        // When reading, the list of fields that are read depends on
        // the version. In the opposite direction, we skip fields
        // which are None and write anything that is Some. We write
        // the bytecode in the same version that was read. If this is
        // ever changed, we'd need to conditonalize here similarly as
        // when reading.

        self._write_int(code.argcount);
        self._maybe_write_int(code.posonlyargcount);
        self._write_int(code.kwonlyargcount);
        self._maybe_write_int(code.nlocals);
        self._write_int(code.stacksize);
        self._write_int(code.flags);
        self.write_object(&code.code);
        self.write_object(&code.consts);
        self.write_object(&code.names);
        self.maybe_write_object(&code.varnames);
        self.maybe_write_object(&code.freevars);
        self.maybe_write_object(&code.cellvars);

        self.maybe_write_object(&code.localsplusnames);
        self.maybe_write_object(&code.localspluskinds);

        self.write_object(&code.filename);
        self.write_object(&code.name);

        self.maybe_write_object(&code.qualname);

        self._write_int(code.firstlineno);

        self.write_object(&code.linetable);
        self.maybe_write_object(&code.exceptiontable);
    }

    fn write_string(&mut self, string: &StringObject) {
        self.buffer.push(
            match string.variant {
                StringVariant::ShortAscii         => b'z',
                StringVariant::ShortAsciiInterned => b'Z',
                StringVariant::String             => b's',
                StringVariant::Interned           => b't',
                StringVariant::Unicode            => b'u',
                StringVariant::Ascii              => b'a',
                StringVariant::AsciiInterned      => b'A',
            }
        );

        let len = string.bytes.len();
        match string.variant {
            // short == size is stored as one byte
            StringVariant::ShortAscii |
            StringVariant::ShortAsciiInterned => {
                self.buffer.push(len as u8);
            }
            // non-short == size is stored as long (4 bytes)
            StringVariant::String |
            StringVariant::Interned |
            StringVariant::Unicode |
            StringVariant::Ascii |
            StringVariant::AsciiInterned => {
                self._write_int(len as u32);
            }
        };

        self.buffer.extend_from_slice(&string.bytes);
    }

    fn write_seq(&mut self, seq: &SeqObject) {
        let len = seq.items.len();
        let byte = match seq.variant {
            SeqVariant::Tuple => {
                if len < 256 {
                    b')'  // SMALL_TUPLE
                } else {
                    b'('  // TUPLE
                }
            }
            SeqVariant::List      => b'[',
            SeqVariant::Set       => b'<',
            SeqVariant::FrozenSet => b'>',
        };

        self.buffer.push(byte);

        if byte == b')' {
            self.buffer.push(len as u8);
        } else {
            self._write_int(len as u32);
        }

        for item in seq.items.iter() {
            self.write_object(item);
        }
    }

    fn write_slice(&mut self, slice: &SliceObject) {
        self.buffer.push(b':');
        self.write_object(&slice.start);
        self.write_object(&slice.stop);
        self.write_object(&slice.step);
    }

    fn _write_int(&mut self, int: u32) {
        let bytes = int.to_le_bytes();
        self.buffer.extend_from_slice(&bytes);
    }

    fn _write_signed_int(&mut self, int: i32) {
        let bytes = int.to_le_bytes();
        self.buffer.extend_from_slice(&bytes);
    }

    fn _maybe_write_int(&mut self, int: Option<u32>) {
        if let Some(int) = int {
            self._write_int(int);
        }
    }

    fn write_int(&mut self, int: u32) {
        self.buffer.push(b'i');
        self._write_int(int);
    }

    fn _write_short(&mut self, int: u16) {
        let bytes = int.to_le_bytes();
        self.buffer.extend_from_slice(&bytes);
    }

    fn write_long(&mut self, long: &BigInt) {
        self.buffer.push(b'l');

        let n = long.bits().div_ceil(PYLONG_MARSHAL_SHIFT as usize);
        let sign = if *long < BigInt::zero() { -1i32 } else { 1i32 };

        self._write_signed_int(n as i32 * sign);

        let mut val = long.abs();
        let div = BigInt::from(1u16 << PYLONG_MARSHAL_SHIFT);
        for _ in 0 .. n {
            let (q, r) = val.div_rem(&div);
            self._write_short(r.to_u16().unwrap());
            val = q;
        }
        assert!(val.is_zero());
    }

    fn _write_binary_float(&mut self, float: u64) {
        let bytes = f64::from_bits(float).to_le_bytes();
        self.buffer.extend_from_slice(&bytes);
    }

    fn write_binary_float(&mut self, float: u64) {
        self.buffer.push(b'g');
        self._write_binary_float(float);
    }

    fn write_binary_complex(&mut self, x: u64, y: u64) {
        self.buffer.push(b'y');
        self._write_binary_float(x);
        self._write_binary_float(y);
    }

    fn write_ref(&mut self, target: Rc<Object>) {
        let offset = self.buffer.len();
        self.buffer.push(b'r');
        self._write_int(0);  // We'll fix the reference number later
        self.refs_to_fix.insert(offset, target);
    }

    fn add_ref_flags(&mut self) {
        let mut keys: Vec<_> = self.seen.keys().collect();
        keys.sort_by_key(|&e| self.seen[e].0);

        for entry in keys {
            let (offset, count, index) = &self.seen[entry];
            assert!(index.borrow().is_none());

            if *count > 0 {
                let orig = self.buffer[*offset];
                if TRACE {
                    debug!("Flagged {:?}, offset {}/{:x}, adding flag #{} ({} refs)",
                           entry, offset, offset, self.flag_num, count);
                }

                assert!("0NFT.ScgilyrzZstuaA)([<>{:".contains(orig as char));
                self.buffer[*offset] |= FLAG_REF_BIT;

                index.replace(Some(self.flag_num));

                self.flag_num += 1;
            }
        }
    }

    fn fix_refs(&mut self) {
        for (offset, target) in &self.refs_to_fix {
            let (target_offset, count, index) = &self.seen[target];
            if TRACE {
                debug!("Ref at offset {}, setting target {}/0x{:x} {:?} #{:?} ({} refs)",
                       offset, target_offset, target_offset, target, index, count);
            }
            assert!(*count > 0);
            let index = index.borrow().unwrap();
            assert!(index < self.flag_num);
            assert!(offset > target_offset);

            assert!(self.buffer[*offset] == b'r');
            let bytes = &mut self.buffer[offset + 1 .. offset + 5];
            assert!(bytes == [0; 4]);
            bytes.copy_from_slice(&(index as u32).to_le_bytes());
        }
    }
}


pub struct Pyc {
    config: Rc<config::Config>,
}

impl Pyc {
    pub fn new(config: &Rc<config::Config>) -> Self {
        Self { config: config.clone() }
    }

    pub fn boxed(config: &Rc<config::Config>) -> Box<dyn super::Processor> {
        Box::new(Self::new(config))
    }
}

impl super::Processor for Pyc {
    fn name(&self) -> &str {
        "pyc"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(self.config.ignore_extension || path.extension().is_some_and(|x| x == "pyc"))
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let (mut io, input) = InputOutputHelper::open(input_path, self.config.check, true)?;

        let mut parser = PycParser::from_file(input_path, input)?;
        if parser.version < (3, 0) {
            return Ok(super::ProcessResult::Noop);  // We don't want to touch python2 files
        }
        if parser.py_content_hash().is_some() {
            return Err(super::Error::Other(
                "pyc file with hash invalidation are not supported".to_string()
            ).into());
        }

        let code = parser.read_object()?;

        let trailing = parser.data.len() - parser.read_offset;
        if trailing > 0 {
            warn!("{}: found trailing garbage ({} bytes)", input_path.display(), trailing);
        }

        let new = PycWriter::to_buffer(&parser, &code);
        let have_mod = new != parser.data;

        if have_mod {
            io.open_output()?;
            io.output.as_mut().unwrap().write_all(&new)?;
        }

        io.finalize(have_mod)
    }
}

impl Pyc {
    pub fn pretty_print<W>(&self, writer: &mut W, input_path: &Path) -> Result<()>
    where
        W: fmt::Write,
    {
        let input = File::open(input_path)
            .with_context(|| format!("Cannot open {:?}", input_path))?;
        let mut parser = PycParser::from_file(input_path, input)?;

        let obj = parser.read_object()?;

        obj.pretty_print(writer, "", "\n", true, true)?;

        Ok(())
    }
}

pub struct PycZeroMtime {
    config: Rc<config::Config>,
}

impl PycZeroMtime {
    pub fn boxed(config: &Rc<config::Config>) -> Box<dyn super::Processor> {
        Box::new(Self { config: config.clone() })
    }

    fn set_zero_mtime_on_py_file(&self, input_path: &Path) -> Result<()> {
        let input_file_name = unwrap_os_string(input_path.file_name().unwrap())?;
        let base = input_file_name.split('.').nth(0).unwrap();
        let py_path = input_path.with_file_name(format!("{base}.py"));
        debug!("Looking at {}…", py_path.display());

        let py_file = match File::open(&py_path) {
            Ok(some) => some,
            Err(e) => {
                if e.kind() == io::ErrorKind::NotFound {
                    debug!("{}: not found, ignoring", py_path.display());
                    return Ok(());
                } else {
                    bail!("{}: cannot open: {}", py_path.display(), e);
                }
            }
        };

        let orig = py_file.metadata()?;
        if !orig.file_type().is_file() {
            debug!("{}: not a file, ignoring", py_path.display());
        } else if orig.modified()? == time::UNIX_EPOCH {
            debug!("{}: mtime is already 0", py_path.display());
        } else if self.config.check {
            debug!("{}: not touching mtime in --check mode", py_path.display());
        } else {
            py_file.set_modified(time::UNIX_EPOCH)?;
            debug!("{}: mtime set to 0", py_path.display());
        }

        Ok(())
    }
}

impl super::Processor for PycZeroMtime {
    fn name(&self) -> &str {
        "pyc-zero-mtime"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(self.config.ignore_extension || path.extension().is_some_and(|x| x == "pyc"))
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let (mut io, input) = InputOutputHelper::open(input_path, self.config.check, false)?;

        let mut parser = PycParser::from_file(input_path, input)?;
        let have_mod = parser.set_zero_mtime()?;

        if have_mod {
            io.open_output()?;
            io.output.as_mut().unwrap().write_all(&parser.data)?;
        }

        let res = io.finalize(have_mod)?;

        if have_mod {
            self.set_zero_mtime_on_py_file(input_path)?;
        }

        Ok(res)
    }
}


#[cfg(test)]
mod tests {
    use std::hash::{DefaultHasher, Hasher};
    use super::*;

    #[test]
    fn filter_a() {
        let cfg = config::Config::empty(0, false).into();
        let h = Pyc::boxed(&cfg);

        assert!( h.filter(Path::new("/some/path/foobar.pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/foobar.apyc")).unwrap());
        assert!( h.filter(Path::new("/some/path/foobar.opt-2.pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/foobar")).unwrap());
        assert!(!h.filter(Path::new("/some/path/pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/pyc_pyc")).unwrap());
        assert!(!h.filter(Path::new("/")).unwrap());
    }

    #[test]
    fn seq_string_equality() {
        let seq1 = Object::Seq(
            SeqObject {
                variant: SeqVariant::FrozenSet,
                items: [
                    Object::String(
                        StringObject {
                            variant: StringVariant::ShortAsciiInterned,
                            bytes: [104, 116, 116, 112].to_vec(),
                            flag_num: Some(43),
                        }
                    ).into(),
                    Object::String(
                        StringObject {
                            variant: StringVariant::ShortAsciiInterned,
                            bytes: [104, 116, 116, 112, 115].to_vec(),
                            flag_num: Some(44),
                        }
                    ).into(),
                ].to_vec(),
                flag_num: None,
            }
        );
        let seq2 = Object::Seq(
            SeqObject {
                variant: SeqVariant::FrozenSet,
                items: [
                    Object::String(
                        StringObject {
                            variant: StringVariant::ShortAsciiInterned,
                            bytes: [104, 116, 116, 112].to_vec(),
                            flag_num: None,
                        }
                    ).into(),
                    Object::String(
                        StringObject {
                            variant: StringVariant::ShortAsciiInterned,
                            bytes: [104, 116, 116, 112, 115].to_vec(),
                            flag_num: None,
                        }
                    ).into(),
                ].to_vec(),
                flag_num: Some(43),
            }
        );

        assert!(seq1 == seq1);
        assert!(seq2 == seq2);
        assert!(seq1 == seq2);
        assert!(seq2 == seq1);

        let mut hash1 = DefaultHasher::new();
        seq1.hash(&mut hash1);

        let mut hash2 = DefaultHasher::new();
        seq2.hash(&mut hash2);

        assert!(hash1.finish() == hash2.finish());
    }

    #[test]
    fn seq_ref_equality() {
        let obj1 = Object::Ref(
            RefObject {
                number: 43,
                target: Object::String(
                    StringObject {
                        variant: StringVariant::ShortAsciiInterned,
                        bytes: [104, 116, 116, 112].to_vec(),
                        flag_num: Some(43),
                    },
                ).into(),
                flag_num: Some(99),
            }
        );
        let obj2 = Object::String(
            StringObject {
                variant: StringVariant::ShortAsciiInterned,
                bytes: [104, 116, 116, 112].to_vec(),
                flag_num: None,
            },
        );

        assert!(obj1 == obj1);
        assert!(obj2 == obj2);
        assert!(obj1 == obj2);
        assert!(obj2 == obj1);
    }
}
