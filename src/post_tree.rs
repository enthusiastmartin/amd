use crate::amd::*;
use crate::internal::*;
use std::fmt::Write;

pub fn post_tree(
    root: i32,
    mut k: i32,
    child: &mut [i32],
    sibling: &[i32],
    order: &mut [i32],
    stack: &mut [i32],
    nn: i32,
) -> i32 {
    /*if false {
        // Recursive version (stack[] is not used):
        // this is simple, but can can cause stack overflow if nn is large
        i = root;
        f = child[i];
        while f != EMPTY {
            k = post_tree(f, k, child, sibling, order, stack, nn);
            f = sibling[f];
        }
        order[i] = k;
        k += 1;
        return k;
    }*/

    // Non-recursive version, using an explicit stack.

    // Push root on the stack.
    let mut head = 0;
    stack[0] = root;

    while head >= 0 {
        // Get head of stack.
        if DEBUG_LEVEL != 0 {
            debug_assert!(head < nn);
        }
        let i = stack[head as usize];
        if DEBUG_LEVEL != 0 {
            print!("head of stack {} \n", i);
            debug_assert!(i >= 0 && i < nn);
        }

        if child[i as usize] != EMPTY {
            // The children of i are not yet ordered
            // push each child onto the stack in reverse order
            // so that small ones at the head of the list get popped first
            // and the biggest one at the end of the list gets popped last.
            let mut f = child[i as usize];
            while f != EMPTY {
                head += 1;
                if DEBUG_LEVEL != 0 {
                    assert!(head < nn);
                    assert!(f >= 0 && f < nn);
                }
                f = sibling[f as usize];
            }
            let mut h = head;
            if DEBUG_LEVEL != 0 {
                debug_assert!(head < nn);
            }
            let mut f = child[i as usize];
            while f != EMPTY {
                if DEBUG_LEVEL != 0 {
                    debug_assert!(h > 0);
                }
                stack[h as usize] = f;
                h -= 1;
                if DEBUG_LEVEL != 0 {
                    println!("push {} on stack", f);
                    debug_assert!(f >= 0 && f < nn);
                }
                f = sibling[f as usize];
            }
            if DEBUG_LEVEL != 0 {
                debug_assert!(stack[h as usize] == i);
            }

            // Delete child list so that i gets ordered next time we see it.
            child[i as usize] = EMPTY;
        } else {
            // The children of i (if there were any) are already ordered
            // remove i from the stack and order it. Front i is kth front.
            head -= 1;
            if DEBUG_LEVEL >= 1 {
                print!("pop {} order {}\n", i, k);
            }
            order[i as usize] = k;
            k += 1;
            if DEBUG_LEVEL != 0 {
                debug_assert!(k <= nn);
            }
        }

        if DEBUG_LEVEL != 0 {
            let mut d1 = String::from("\nStack:");
            // for h := head; h >= 0; h-- {
            let mut h = head;
            while h >= 0 {
                let j = stack[h as usize];
                write!(d1, " {}", j).unwrap();
                assert!(j >= 0 && j < nn);
                h -= 1;
            }
            print!("{}\n\n", d1);
            assert!(head < nn);
        }
    }

    return k;
}
