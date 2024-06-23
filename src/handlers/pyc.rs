/* SPDX-License-Identifier: GPL-3.0-or-later */

use anyhow::{anyhow, Result};
use log::debug;
use std::io::{Read, Write};
use std::iter;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::str;

use itertools::Itertools;
use num_bigint_dig::{BigInt, ToBigInt};

use crate::handlers::InputOutputHelper;
use crate::options;

const PYLONG_MARSHAL_SHIFT: i32 = 15;

const TRACE: bool = false;

pub fn pyc_python_version(input_path: &Path, buf: &[u8; 4]) -> Result<((u32, u32), usize)> {
    // https://github.com/python/cpython/blob/main/Lib/importlib/_bootstrap_external.py#L247
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

    //     Python 3.15 will start with 3700

    if buf[2..] != [0x0D, 0x0A] {
        return Err(anyhow!("{}: not a pyc file, wrong magic ({:?})", input_path.display(), buf));
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
        3600..=3699 => Ok(((3, 14), 16)),
        3700..=4000 => Ok(((3, 15), 16)),
        _ => Err(anyhow!("{}: not a pyc file, unknown version ({:?})", input_path.display(), buf)),
    }
}

pub struct Pyc {}

impl Pyc {
    pub fn boxed(_config: &Rc<options::Config>) -> Box<dyn super::Processor> {
        Box::new(Self {})
    }
}

#[derive(Debug)]
#[allow(dead_code)]   // Right now, we only use dbg! to print the object.
enum Object {
    Code {
        argcount: u32,
        posonlyargcount: Option<u32>,
        kwonlyargcount: u32,
        nlocals: Option<u32>,
        stacksize: u32,
        flags: u32,
        code: Box<Object>,
        consts: Box<Object>,
        names: Box<Object>,
        varnames: Option<Box<Object>>,
        freevars: Option<Box<Object>>,
        cellvars: Option<Box<Object>>,
        localsplusnames: Option<Box<Object>>,
        localspluskinds: Option<Box<Object>>,
        filename: Box<Object>,
        name: Box<Object>,
        qualname: Option<Box<Object>>,
        firstlineno: u32,
        linetable: Box<Object>,
        exceptiontable: Option<Box<Object>>,
    },
    //    String(std::string::String),
    Long(BigInt),
    Int(u32),
    Short(i32),
    String(Vec<u8>),
    Tuple(Vec<Object>),
    Null,
    None,
    True,
    False,
    StopIteration,
    Ellipsis,
    Float(f64),
    Complex(f64, f64),
    Dict(Vec<(Object, Object)>),
    Ref(String),
}

#[derive(Debug, Ord, Eq, PartialOrd, PartialEq)]
struct Ref {
    offset: usize,
    number: u64, // This is either the reference count (for flag refs)
                 // or the index into flag ref vector (for serialized refs).
                 // I tried to make this an enum, but I didn't know how to
                 // combine the two lists in one vector later on.
}

pub struct PycParser {
    input_path: PathBuf,
    pub version: (u32, u32),

    data: Vec<u8>,      // the whole contents of the input file
    read_offset: usize, // index into .data

    irefs: Vec<Ref>,
    flag_refs: Vec<Ref>,
}

impl PycParser {
    pub fn from_file(input_path: &Path, mut input: impl Read) -> Result<Self> {
        let mut buf = [0; 4];
        input.read_exact(&mut buf)?;

        let (version, header_length) = pyc_python_version(input_path, &buf)?;
        debug!("{}: pyc file for Python {}.{}", input_path.display(), version.0, version.1);
        if TRACE {
            debug!("{}: pyc file header is {} bytes", input_path.display(), header_length);
        }

        let mut data = Vec::from(&buf);
        input.read_to_end(&mut data)?;

        Ok(PycParser {
            input_path: input_path.to_path_buf(),
            version,
            data,
            read_offset: header_length,
            irefs: Vec::new(),
            flag_refs: Vec::new(),
        })
    }

    fn take(&mut self, count: usize) -> Result<usize> {
        // This just checks availability and moves the offset.
        // The return value points to the beginning of data.

        if self.read_offset + count <= self.data.len() {
            let offset = self.read_offset;
            self.read_offset += count;
            Ok(offset)
        } else {
            Err(anyhow!("{}:{}: cannot take {} bytes",
                        self.input_path.display(), self.read_offset, count))
        }
    }

    fn _read_byte(&mut self) -> Result<(usize, u8)> {
        let offset = self.take(1)?;
        Ok((offset, self.data[offset]))
    }

    fn read_object(&mut self) -> Result<Object> {
        let (offset, mut b) = self._read_byte()?;

        if (b & (0x1 << 7)) != 0 {
            b &= !(0x1 << 7);

            self.flag_refs.push(Ref { offset, number: 0 });
        }

        if TRACE {
            debug!("{}:{}: type {}", self.input_path.display(), offset, b);
        }

        let obj = match b {
            b'0' => Object::Null,
            b'N' => Object::None,
            b'F' => Object::False,
            b'T' => Object::True,
            b'.' => Object::Ellipsis,
            b'S' => Object::StopIteration,

            b'c'    // CODE
                => self.read_codeobject()?,
            b'g'    // BINARY_FLOAT
                => self.read_binary_float()?,
            b'i'    // INT
                => self.read_long()?,
            b'l'    // LONG
                => self.read_py_long()?,

            b'r'    // REF
                => self.read_ref(offset)?,

            b's' |  // STRING
            b't' |  // INTERNED
            b'u' |  // UNICODE
            b'a' |  // ASCII
            b'A'    // ASCII_INTERNED
                => self.read_string(false)?,

            b'y'    // BINARY_COMPLEX
                => self.read_binary_complex()?,

            b'z' |  // SHORT_ASCII
            b'Z'    // SHORT_ASCII_INTERNED
                => self.read_string(true)?,

            b')'    // SMALL_TUPLE
                => self.read_small_tuple()?,

            b'(' |  // TUPLE
            b'[' |  // LIST
            b'<' |  // SET
            b'>'    // FROZEN_SET
                => self.read_seq(b)?,

            b'{'    // DICT
                => self.read_dict()?,

            b'I' |  // INT64
            b'f' |  // FLOAT
            b'x' |  // COMPLEX
            b'?'    // UNKNOWN
                => {
                    return Err(anyhow!("Unimplemented object type '{}'", b));
                },
            _
                => {
                    return Err(anyhow!("Unknown object type '{}'", b));
                },
        };

        if TRACE {
            dbg!(&obj);
        }

        Ok(obj)
    }

    fn maybe_read_long(&mut self, cond: bool) -> Result<Option<u32>> {
        Ok(if cond { Some(self._read_long()?) } else { None })
    }

    fn maybe_read_object(&mut self, cond: bool) -> Result<Option<Box<Object>>> {
        Ok(if cond {
            Some(Box::new(self.read_object()?))
        } else {
            None
        })
    }

    fn read_codeobject(&mut self) -> Result<Object> {
        Ok(Object::Code {
            argcount: self._read_long()?,
            posonlyargcount: self.maybe_read_long(self.version >= (3, 8))?,
            kwonlyargcount: self._read_long()?,
            nlocals: self.maybe_read_long(self.version < (3, 11))?,
            stacksize: self._read_long()?,
            flags: self._read_long()?,
            code: Box::new(self.read_object()?),
            consts: Box::new(self.read_object()?),
            names: Box::new(self.read_object()?),
            varnames: self.maybe_read_object(self.version < (3, 11))?,
            freevars: self.maybe_read_object(self.version < (3, 11))?,
            cellvars: self.maybe_read_object(self.version < (3, 11))?,
            localsplusnames: self.maybe_read_object(self.version >= (3, 11))?,
            localspluskinds: self.maybe_read_object(self.version >= (3, 11))?,
            filename: Box::new(self.read_object()?),
            name: Box::new(self.read_object()?),
            qualname: self.maybe_read_object(self.version >= (3, 11))?,
            firstlineno: self._read_long()?,
            linetable: Box::new(self.read_object()?),
            exceptiontable: self.maybe_read_object(self.version >= (3, 11))?,
        })
    }

    fn _read_long(&mut self) -> Result<u32> {
        let offset = self.take(4)?;
        let bytes = &self.data[offset .. offset + 4];
        Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn _read_long_signed(&mut self) -> Result<i32> {
        let offset = self.take(4)?;
        let bytes = &self.data[offset .. offset + 4];
        Ok(i32::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_long(&mut self) -> Result<Object> {
        Ok(Object::Int(self._read_long()?))
    }

    fn _read_short(&mut self) -> Result<i32> {
        let offset = self.take(2)?;

        let x = (self.data[offset] as i32) + ((self.data[offset + 1] as i32) << 8);
        // Sign-extension, in case short greater than 16 bits
        Ok(x | -(x & 0x8000))
    }

    fn read_py_long(&mut self) -> Result<Object> {
        let n = self._read_long_signed()?;

        let mut result = 0_i32.to_bigint().unwrap();
        for i in 0 .. n.abs() {
            let part = self._read_short()?;
            result += part.to_bigint().unwrap() << (i * PYLONG_MARSHAL_SHIFT) as usize;
        }

        Ok(Object::Long(result * n.signum()))
    }

    fn read_string(&mut self, short: bool) -> Result<Object> {
        let size = if short {
            // short == size is stored as one byte
            self._read_byte()?.1 as usize
        } else {
            // non-short == size is stored as long (4 bytes)
            self._read_long()? as usize
        };

        let offset = self.take(size)?;
        Ok(Object::String(self.data[offset .. offset + size].to_vec()))

        // let string = str::from_utf8(&bytes)?;
        // Ok(Object::String(string.to_string()))
    }

    fn _read_tuple(&mut self, size: u64) -> Result<Object> {
        let mut ans = Vec::new();
        for _ in 0..size {
            ans.push(self.read_object()?);
        }

        Ok(Object::Tuple(ans))
    }

    fn read_small_tuple(&mut self) -> Result<Object> {
        // small tuple — size is only one byte
        let size = self._read_byte()?.1;
        self._read_tuple(size as u64)
    }

    fn read_seq(&mut self, _typ: u8) -> Result<Object> {
        let size = self._read_long()?;
        self._read_tuple(size as u64)
    }

    fn read_ref(&mut self, offset: usize) -> Result<Object> {
        let index = self._read_long()?;

        // Is this a valid reference to one of the already-flagged objects?
        if index as usize >= self.flag_refs.len() {
            return Err(anyhow!("{}:0x{:x}: bad reference to flag_ref {} (have {})",
                               self.input_path.display(), self.read_offset,
                               index, self.flag_refs.len()));
        }

        self.flag_refs[index as usize].number += 1;
        self.irefs.push(Ref { offset, number: index as u64 });

        let desc = format!("REF to index {}", index);
        Ok(Object::Ref(desc))
    }

    fn _read_binary_float(&mut self) -> Result<f64> {
        let offset = self.take(8)?;
        let bytes = &self.data[offset .. offset + 8];
        Ok(f64::from_le_bytes(bytes.try_into().unwrap()))
    }

    fn read_binary_float(&mut self) -> Result<Object> {
        Ok(Object::Float(self._read_binary_float()?))
    }

    fn read_binary_complex(&mut self) -> Result<Object> {
        Ok(Object::Complex(self._read_binary_float()?,
                           self._read_binary_float()?))
    }

    fn read_dict(&mut self) -> Result<Object> {
        let mut dict = Vec::new();

        loop {
            let key = self.read_object()?;
            if let Object::Null = key {
                break;
            }

            let value = self.read_object()?;
            dict.push((key, value));
        }

        Ok(Object::Dict(dict))
    }

    fn clear_unused_flag_refs(&mut self) -> Result<(bool, Vec<u8>)> {
        // Sequence of flag_refs and irefs ordered by number of byte in a file
        let final_list =
            iter::zip(self.flag_refs.iter(), iter::repeat(true))
            .merge_by(iter::zip(self.irefs.iter(), iter::repeat(false)),
                      |&a, &b| a.0.offset < b.0.offset);

        // A map where at a beginning, index in list == number of flag_ref
        // but when unused flag is removed:
        // - numbers in the list are original numbers of flag_refs
        // - indices of the list are new numbers
        let flag_ref_count = self.flag_refs.len();
        let mut flag_ref_map = (0..flag_ref_count).collect::<Vec<_>>();

        // new mutable content
        let mut data = self.data.clone();

        let mut removed_count = 0;
        for (r, is_flag) in final_list {
            // Clear FLAG_REF bit and remove it from map.
            // all subsequent refs will have lower index in the map.
            if is_flag {
                if r.number == 0 {
                    let index = self.flag_refs.binary_search(r).unwrap();
                    let index_index = flag_ref_map.binary_search(&index).unwrap();
                    flag_ref_map.remove(index_index);
                    assert!(data[r.offset] & (1 << 7) > 0);
                    data[r.offset] &= !(1 << 7);
                    removed_count += 1;
                }
            } else {
                let index = r.number as usize;
                let new_index = flag_ref_map.binary_search(&index);
                if new_index.is_err() {
                    dbg!(&flag_ref_map);
                    dbg!(&r.number);
                }
                let new_index = new_index.unwrap() as u32;

                data[r.offset + 1 .. r.offset + 5]
                    .copy_from_slice(&new_index.to_le_bytes());
            }
        }

        debug!("{}: removed {} unused FLAG_REFs", self.input_path.display(), removed_count);
        assert_eq!(data == self.data, removed_count == 0);
        Ok((removed_count > 0, data))
    }
}

impl super::Processor for Pyc {
    fn name(&self) -> &str {
        "pyc"
    }

    fn filter(&self, path: &Path) -> Result<bool> {
        Ok(path.extension().is_some_and(|x| x == "pyc"))
    }

    fn process(&self, input_path: &Path) -> Result<super::ProcessResult> {
        let (mut io, input) = InputOutputHelper::open(input_path)?;

        let mut parser = PycParser::from_file(input_path, input)?;
        if parser.version < (3, 0) {
            return Ok(super::ProcessResult::Noop);  // We don't want to touch python2 files
        }

        parser.read_object()?;

        let (have_mod, data) = parser.clear_unused_flag_refs()?;
        if have_mod {
            io.open_output()?;
            io.output.as_mut().unwrap().write_all(&data)?;
        }

        io.finalize(have_mod)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filter_a() {
        let cfg = Rc::new(options::Config::empty(0));
        let h = Pyc::boxed(&cfg);

        assert!( h.filter(Path::new("/some/path/foobar.pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/foobar.apyc")).unwrap());
        assert!( h.filter(Path::new("/some/path/foobar.opt-2.pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/foobar")).unwrap());
        assert!(!h.filter(Path::new("/some/path/pyc")).unwrap());
        assert!(!h.filter(Path::new("/some/path/pyc_pyc")).unwrap());
        assert!(!h.filter(Path::new("/")).unwrap());
    }
}
