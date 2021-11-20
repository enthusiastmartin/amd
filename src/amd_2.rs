use crate::amd::*;
#[cfg(feature = "debug1")]
use crate::dump::dump;
use crate::internal::*;
use crate::postorder::postorder;
use std::cmp::{max, min};

fn clear_flag(wflg: i32, wbig: i32, w: &mut [i32], n: i32) -> i32 {
    if wflg < 2 || wflg >= wbig {
        for x in 0..n {
            if w[x as usize] != 0 {
                w[x as usize] = 1;
            }
        }
        return 2;
    }
    // At this point, W[0..n-1] < wflg holds.
    return wflg;
}

pub fn amd_2(
    n: i32,
    pe: &mut [i32],  // input/output
    iw: &mut [i32],  // input/modified (undefined on output)
    len: &mut [i32], // input/modified (undefined on output)
    iwlen: i32,
    mut pfree: i32,
    control: &Control,
    info: &mut Info,
) -> (Vec<i32>, Vec<i32>, Vec<i32>, Vec<i32>) {
    // local workspace (not input or output - used only during execution)
    let mut head: Vec<i32> = vec![0; n as usize];
    let mut degree: Vec<i32> = vec![0; n as usize];
    let mut w: Vec<i32> = vec![0; n as usize];

    // output
    let mut nv: Vec<i32> = vec![0; n as usize];
    let mut next: Vec<i32> = vec![0; n as usize];
    let mut last: Vec<i32> = vec![0; n as usize];
    let mut e_len: Vec<i32> = vec![0; n as usize];

    let mut hash: u32; // unsigned, so that hash % n is well defined.

    // Any parameter (Pe[...] or pfree) or local variable starting with "p" (for
    // Pointer) is an index into Iw, and all indices into Iw use variables starting
    // with "p". The only exception to this rule is the iwlen input argument.

    // Initializations

    // Note that this restriction on iwlen is slightly more restrictive than
    // what is actually required in amd_2. amd_2 can operate with no elbow
    // room at all, but it will be slow. For better performance, at least
    // size-n elbow room is enforced.
    debug_assert!(iwlen >= pfree + n);
    debug_assert!(n > 0);

    /* Initialize Output Statistics */

    // The number of nonzeros in L (excluding the diagonal).
    let mut lnz = 0;
    // Number of divisions for LU or LDL' factorizations.
    let mut ndiv = 0;
    // Number of multiply-subtract pairs for LU factorization.
    let mut nms_lu = 0;
    // Number of multiply-subtract pairs for LDL' factorization.
    let mut nms_ldl = 0;
    // The largest number of entries in any column of L, including the diagonal.
    let mut dmax = 1;
    // Current supervariable being eliminated, and the current
    // element created by eliminating that supervariable.
    let mut me = EMPTY;

    let mut mindeg = 0; // Current minimum degree.
    let mut ncmpa = 0; // Number of garbage collections.
    let mut nel = 0; // Number of pivots selected so far.
    let mut lemax = 0; // Largest |Le| seen so far (called dmax in Fortran version).

    // Get control parameters.
    let aggressive = if control.aggressive { 1 } else { 0 };
    // "dense" degree ratio.
    let alpha = control.dense;
    // Note: if alpha is NaN, this is undefined:
    let mut dense = if alpha < 0.0 {
        // Only remove completely dense rows/columns.
        n - 2
    } else {
        (alpha * (n as f64).sqrt()) as i32
    };
    dense = max(16, dense);
    let dense = min(n, dense);
    debug1_print!("\n\nAMD (debug), alpha {}, aggr. {}\n", alpha, aggressive);

    for i in 0..n {
        last[i as usize] = EMPTY;
        head[i as usize] = EMPTY;
        next[i as usize] = EMPTY;
        // if separate Hhead array is used for hash buckets:
        //   Hhead[i] = EMPTY
        nv[i as usize] = 1;
        w[i as usize] = 1;
        e_len[i as usize] = 0;
        degree[i as usize] = len[i as usize];
    }

    debug1_print!("\n======Nel {} initial\n", nel);
    #[cfg(feature = "debug1")]
    dump(
        n, pe, iw, len, iwlen, pfree, &nv, &next, &last, &head, &e_len, &degree, &w, -1,
    );

    // INT_MAX - n for the int version, UF_long_max - n for the
    // int64 version. wflg is not allowed to be >= wbig.
    let wbig = std::i32::MAX - n;
    // Used for flagging the W array. See description of Iw.
    let mut wflg = clear_flag(0, wbig, &mut w, n);

    // Initialize degree lists and eliminate dense and empty rows.

    let mut ndense = 0; // Number of "dense" rows/columns.

    for i in 0..n {
        let deg = degree[i as usize]; // The degree of a variable or element.

        debug_assert!(deg >= 0 && deg < n);
        if deg == 0 {
            // We have a variable that can be eliminated at once because
            // there is no off-diagonal non-zero in its row. Note that
            // Nv [i] = 1 for an empty variable i. It is treated just
            // the same as an eliminated element i.

            e_len[i as usize] = flip(1);
            nel += 1;
            pe[i as usize] = EMPTY;
            w[i as usize] = 0;
        } else if deg > dense {
            // Dense variables are not treated as elements, but as unordered,
            // non-principal variables that have no parent. They do not take
            // part in the postorder, since Nv [i] = 0. Note that the Fortran
            // version does not have this option.

            debug1_print!("Dense node {} degree {}\n", i, deg);
            ndense += 1;
            nv[i as usize] = 0;
            // do not postorder this node
            e_len[i as usize] = EMPTY;
            nel += 1;
            pe[i as usize] = EMPTY
        } else {
            // Place i in the degree list corresponding to its degree.

            let inext = head[deg as usize]; // The entry in a link list following i.

            debug_assert!(inext >= EMPTY && inext < n);
            if inext != EMPTY {
                last[inext as usize] = i;
            }
            next[i as usize] = inext;
            head[deg as usize] = i;
        }
    }

    // While (selecting pivots) do.
    while nel < n {
        debug1_print!("\n======Nel {}\n", nel);
        #[cfg(feature = "debug2")]
        dump(
            n, pe, iw, len, iwlen, pfree, &nv, &next, &last, &head, &e_len, &degree, &w, nel,
        );

        // Get pivot of minimum degree.

        // Find next supervariable for elimination.

        debug_assert!(mindeg >= 0 && mindeg < n);
        let mut deg = mindeg; // The degree of a variable or element.
        while deg < n {
            me = head[deg as usize];
            if me != EMPTY {
                break;
            }
            deg += 1;
        }
        mindeg = deg;
        debug_assert!(me >= 0 && me < n);
        debug1_print!("=================me: {}\n", me);

        // Remove chosen variable from link list.
        let mut inext = next[me as usize]; // The entry in a link list following i.

        debug_assert!(inext >= EMPTY && inext < n);
        if inext != EMPTY {
            last[inext as usize] = EMPTY;
        }
        head[deg as usize] = inext;

        // me represents the elimination of pivots nel to nel+Nv[me]-1.
        // place me itself as the first in this set.
        let elenme = e_len[me as usize]; // The length, Elen [me], of element list of pivotal variable.
        let mut nvpiv = nv[me as usize]; // Number of pivots in current element.

        debug_assert!(nvpiv > 0);
        nel += nvpiv;

        // Construct new element.

        // At this point, me is the pivotal supervariable. It will be
        // converted into the current element. Scan list of the pivotal
        // supervariable, me, setting tree pointers and constructing new list
        // of supervariables for the new element, me. p is a pointer to the
        // current position in the old list.

        // Flag the variable "me" as being in Lme by negating Nv[me].
        nv[me as usize] = -nvpiv;
        let mut degme = 0; // Size, |Lme|, of the current element, me (= degree[me]).

        debug_assert!(pe[me as usize] >= 0 && pe[me as usize] < iwlen);

        let mut pme1: i32; // The current element, me, is stored in Iw[pme1...pme2].
        let mut pme2: i32; // The end of the current element.
        if elenme == 0 {
            // Construct the new element in place.
            pme1 = pe[me as usize];
            pme2 = pme1 - 1;

            for p in pme1..=pme1 + len[me as usize] - 1 {
                let i = iw[p as usize];
                debug_assert!(i >= 0 && i < n && nv[i as usize] >= 0);

                let nvi = nv[i as usize]; // The number of variables in a supervariable i (= Nv[i])
                if nvi > 0 {
                    // i is a principal variable not yet placed in Lme.
                    // store i in new list

                    // Flag i as being in Lme by negating Nv[i].
                    degme += nvi;
                    nv[i as usize] = -nvi;
                    pme2 += 1;
                    iw[pme2 as usize] = i;

                    // Remove variable i from degree list.
                    let ilast = last[i as usize]; // The entry in a link list preceding i.
                    inext = next[i as usize];
                    debug_assert!(ilast >= EMPTY && ilast < n);
                    debug_assert!(inext >= EMPTY && inext < n);

                    if inext != EMPTY {
                        last[inext as usize] = ilast;
                    }
                    if ilast != EMPTY {
                        next[ilast as usize] = inext;
                    } else {
                        // i is at the head of the degree list.
                        debug_assert!(degree[i as usize] >= 0 && degree[i as usize] < n);
                        head[degree[i as usize] as usize] = inext;
                    }
                }
            }
        } else {
            // Construct the new element in empty space, Iw[pfree ...]
            let mut p = pe[me as usize];
            pme1 = pfree;
            // Number of variables in variable list of pivotal variable.
            let slenme = len[me as usize] - elenme;

            for knt1 in 1..=elenme + 1 {
                let e: i32;
                let mut pj: i32;
                let ln: i32;
                if knt1 > elenme {
                    // Search the supervariables in me.
                    e = me;
                    pj = p;
                    ln = slenme;
                    debug2_print!("Search sv: {} {} {}\n", me, pj, ln);
                } else {
                    // Search the elements in me.
                    e = iw[p as usize];
                    p += 1;
                    debug_assert!(e >= 0 && e < n);

                    pj = pe[e as usize];
                    ln = len[e as usize];

                    debug2_print!("Search element e {} in me {}\n", e, me);
                    debug_assert!(e_len[e as usize] < EMPTY && w[e as usize] > 0 && pj >= 0);
                }
                debug_assert!(ln >= 0 && (ln == 0 || (pj >= 0 && pj < iwlen)));

                // search for different supervariables and add them to the
                // new list, compressing when necessary. this loop is
                // executed once for each element in the list and once for
                // all the supervariables in the list.

                for knt2 in 1..=ln {
                    let i = iw[pj as usize];
                    pj += 1;
                    debug_assert!(i >= 0 && i < n && (i == me || e_len[i as usize] >= EMPTY));

                    // The number of variables in a supervariable i (= Nv[i]).
                    let nvi = nv[i as usize];
                    debug2_print!(
                        ": {} {} {} {}\n",
                        i,
                        e_len[i as usize],
                        nv[i as usize],
                        wflg
                    );

                    if nvi > 0 {
                        // Compress Iw, if necessary.
                        if pfree >= iwlen {
                            debug1_println!("GARBAGE COLLECTION");

                            // Prepare for compressing Iw by adjusting pointers
                            // and lengths so that the lists being searched in
                            // the inner and outer loops contain only the
                            // remaining entries.

                            pe[me as usize] = p;
                            len[me as usize] -= knt1;
                            // Check if nothing left of supervariable me.
                            if len[me as usize] == 0 {
                                pe[me as usize] = EMPTY;
                            }
                            pe[e as usize] = pj;
                            len[e as usize] = ln - knt2;
                            // Nothing left of element e.
                            if len[e as usize] == 0 {
                                pe[e as usize] = EMPTY;
                            }

                            ncmpa += 1; // One more garbage collection.

                            // Store first entry of each object in Pe
                            // flip the first entry in each object
                            for j in 0..n {
                                let pn = pe[j as usize];
                                if pn >= 0 {
                                    debug_assert!(pn >= 0 && pn < iwlen);

                                    pe[j as usize] = iw[pn as usize];
                                    iw[pn as usize] = flip(j);
                                }
                            }

                            // psrc/pdst point to source/destination
                            let mut psrc = 0;
                            let mut pdst = 0;
                            let pend = pme1 - 1;

                            while psrc <= pend {
                                // Search for next flip'd entry.
                                let j = flip(iw[psrc as usize]);
                                psrc += 1;
                                if j >= 0 {
                                    debug2_print!("Got object j: {}\n", j);

                                    iw[pdst as usize] = pe[j as usize];
                                    pe[j as usize] = pdst;
                                    pdst += 1;
                                    let lenj = len[j as usize];
                                    // Copy from source to destination.
                                    for _knt3 in 0..=lenj - 2 {
                                        iw[pdst as usize] = iw[psrc as usize];
                                        pdst += 1;
                                        psrc += 1;
                                    }
                                }
                            }

                            // Move the new partially-constructed element.
                            let p1 = pdst;
                            psrc = pme1;
                            while psrc <= pfree - 1 {
                                iw[pdst as usize] = iw[psrc as usize];
                                pdst += 1;
                                psrc += 1;
                            }
                            pme1 = p1;
                            pfree = pdst;
                            pj = pe[e as usize];
                            p = pe[me as usize];
                        }

                        // i is a principal variable not yet placed in Lme
                        // store i in new list.

                        // Flag i as being in Lme by negating Nv[i].
                        degme += nvi;
                        nv[i as usize] = -nvi;
                        iw[pfree as usize] = i;
                        pfree += 1;
                        debug2_print!("     s: {}     nv {}\n", i, nv[i as usize]);

                        // Remove variable i from degree link list.

                        let ilast = last[i as usize]; // The entry in a link list preceding i.
                        inext = next[i as usize];
                        debug_assert!(ilast >= EMPTY && ilast < n);
                        debug_assert!(inext >= EMPTY && inext < n);

                        if inext != EMPTY {
                            last[inext as usize] = ilast;
                        }
                        if ilast != EMPTY {
                            next[ilast as usize] = inext;
                        } else {
                            // i is at the head of the degree list.
                            debug_assert!(degree[i as usize] >= 0 && degree[i as usize] < n);

                            head[degree[i as usize] as usize] = inext;
                        }
                    }
                }

                if e != me {
                    // Set tree pointer and flag to indicate element e is
                    // absorbed into new element me (the parent of e is me).
                    debug1_print!(" Element {} => {}\n", e, me);

                    pe[e as usize] = flip(me);
                    w[e as usize] = 0;
                }
            }

            pme2 = pfree - 1;
        }

        // me has now been converted into an element in Iw[pme1..pme2]

        // degme holds the external degree of new element.
        degree[me as usize] = degme;
        pe[me as usize] = pme1;
        len[me as usize] = pme2 - pme1 + 1;
        debug_assert!(pe[me as usize] >= 0 && pe[me as usize] < iwlen);

        e_len[me as usize] = flip(nvpiv + degme);
        // flip(Elen(me)) is now the degree of pivot (including diagonal part).

        debug2_print!("New element structure: length={}\n", pme2 - pme1 + 1);
        #[cfg(feature = "debug3")]
        for pme in pme1..=pme2 {
            debug3_print!(" {}", iw[pme as usize]);
        }
        debug3_println!();

        // Make sure that wflg is not too large.

        // With the current value of wflg, wflg+n must not cause integer overflow.

        wflg = clear_flag(wflg, wbig, &mut w, n);

        // compute(W [e] - wflg) = |Le\Lme| for all elements.

        // Scan 1:  compute the external degrees of previous elements with
        // respect to the current element. That is:
        //       (W [e] - wflg) = |Le \ Lme|
        // for each element e that appears in any supervariable in Lme. The
        // notation Le refers to the pattern (list of supervariables) of a
        // previous element e, where e is not yet absorbed, stored in
        // Iw [Pe [e] + 1 ... Pe [e] + Len [e]]. The notation Lme
        // refers to the pattern of the current element (stored in
        // Iw [pme1..pme2]).  If aggressive absorption is enabled, and
        // (W [e] - wflg) becomes zero, then the element e will be absorbed
        // in Scan 2.

        debug2_print!("me: ");
        for pme in pme1..=pme2 {
            let i = iw[pme as usize];
            debug_assert!(i >= 0 && i < n);

            let eln = e_len[i as usize]; // The length, Elen[...], of an element list.
            debug3_print!("{} Elen {}: \n", i, eln);

            if eln > 0 {
                // Note that Nv[i] has been negated to denote i in Lme:
                let nvi = -nv[i as usize];
                debug_assert!(nvi > 0 && pe[i as usize] >= 0 && pe[i as usize] < iwlen);

                let wnvi = wflg - nvi;
                for p in pe[i as usize]..=pe[i as usize] + eln - 1 {
                    let e = iw[p as usize];
                    debug_assert!(e >= 0 && e < n);

                    let mut we = w[e as usize];
                    debug4_print!("    e {} we {} ", e, we);

                    if we >= wflg {
                        // Unabsorbed element e has been seen in this loop.
                        debug4_print!("    unabsorbed, first time seen");
                        we -= nvi;
                    } else if we != 0 {
                        // e is an unabsorbed element.
                        // This is the first we have seen e in all of Scan 1.
                        debug4_print!("    unabsorbed");
                        we = degree[e as usize] + wnvi;
                    }
                    debug4_println!();
                    w[e as usize] = we;
                }
            }
        }
        debug2_println!();

        // Degree update and element absorption.

        // Scan 2:  for each i in Lme, sum up the degree of Lme (which is
        // degme), plus the sum of the external degrees of each Le for the
        // elements e appearing within i, plus the supervariables in i.
        // Place i in hash list.

        for pme in pme1..=pme2 {
            let i = iw[pme as usize];
            debug_assert!(i >= 0 && i < n && nv[i as usize] < 0 && e_len[i as usize] >= 0);
            debug2_print!(
                "Updating: i {} {} {}\n",
                i,
                e_len[i as usize],
                len[i as usize]
            );

            let p1 = pe[i as usize];
            let p2 = p1 + e_len[i as usize] - 1;
            let mut pn = p1;
            hash = 0;
            deg = 0;
            debug_assert!(p1 >= 0 && p1 < iwlen && p2 >= -1 && p2 < iwlen);

            // scan the element list associated with supervariable i .

            // UMFPACK/MA38-style approximate degree:
            if aggressive != 0 {
                for p in p1..=p2 {
                    let e = iw[p as usize];
                    debug_assert!(e >= 0 && e < n);

                    let we = w[e as usize];
                    if we != 0 {
                        // e is an unabsorbed element.
                        let dext = we - wflg; // External degree, |Le \ Lme|, of some element e.
                        if dext > 0 {
                            deg += dext;
                            iw[pn as usize] = e;
                            pn += 1;
                            hash += e as u32;
                            debug4_print!(" e: {} hash = {}\n", e, hash);
                        } else {
                            // External degree of e is zero, absorb e into me.
                            debug4_print!(" Element {} => {} (aggressive)\n", e, me);
                            debug_assert!(dext == 0);
                            pe[e as usize] = flip(me);
                            w[e as usize] = 0;
                        }
                    }
                }
            } else {
                for p in p1..=p2 {
                    let e = iw[p as usize];
                    debug_assert!(e >= 0 && e < n);
                    let we = w[e as usize];
                    if we != 0 {
                        // e is an unabsorbed element.
                        let dext = we - wflg;
                        debug_assert!(dext >= 0);
                        deg += dext;
                        iw[pn as usize] = e;
                        pn += 1;
                        hash += e as u32;
                        debug4_print!(" e: {} hash = {}\n", e, hash);
                    }
                }
            }

            // Count the number of elements in i (including me):
            e_len[i as usize] = pn - p1 + 1;

            // Scan the supervariables in the list associated with i.

            // The bulk of the AMD run time is typically spent in this loop,
            // particularly if the matrix has many dense rows that are not
            // removed prior to ordering.
            let p3 = pn;
            let p4 = p1 + len[i as usize];
            for p in p2 + 1..p4 {
                let j = iw[p as usize];
                debug_assert!(j >= 0 && j < n);

                let nvj = nv[j as usize];
                if nvj > 0 {
                    // j is unabsorbed, and not in Lme.
                    // Add to degree and add to new list.
                    deg += nvj;
                    iw[pn as usize] = j;
                    pn += 1;
                    hash += j as u32;
                    debug4_print!("  s: {} hash {} Nv[j]= {}\n", j, hash, nvj);
                }
            }

            // Update the degree and check for mass elimination.

            // With aggressive absorption, deg==0 is identical to the
            // Elen [i] == 1 && p3 == pn test, below.
            debug_assert!(implies(
                aggressive != 0,
                (deg == 0) == (e_len[i as usize] == 1 && p3 == pn)
            ));

            if e_len[i as usize] == 1 && p3 == pn {
                // Mass elimination

                // There is nothing left of this node except for an edge to
                // the current pivot element. Elen [i] is 1, and there are
                // no variables adjacent to node i. Absorb i into the
                // current pivot element, me. Note that if there are two or
                // more mass eliminations, fillin due to mass elimination is
                // possible within the nvpiv-by-nvpiv pivot block. It is this
                // step that causes AMD's analysis to be an upper bound.
                //
                // The reason is that the selected pivot has a lower
                // approximate degree than the true degree of the two mass
                // eliminated nodes. There is no edge between the two mass
                // eliminated nodes. They are merged with the current pivot
                // anyway.
                //
                // No fillin occurs in the Schur complement, in any case,
                // and this effect does not decrease the quality of the
                // ordering itself, just the quality of the nonzero and
                // flop count analysis. It also means that the post-ordering
                // is not an exact elimination tree post-ordering.

                debug1_print!("  MASS i {} => parent e {}\n", i, me);
                pe[i as usize] = flip(me);
                let nvi = -nv[i as usize];
                degme -= nvi;
                nvpiv += nvi;
                nel += nvi;
                nv[i as usize] = 0;
                e_len[i as usize] = EMPTY;
            } else {
                // Update the upper-bound degree of i.

                // The following degree does not yet include the size
                // of the current element, which is added later:

                degree[i as usize] = min(degree[i as usize], deg);

                // Add me to the list for i.

                // Move first supervariable to end of list.
                iw[pn as usize] = iw[p3 as usize];
                // Move first element to end of element part of list.
                iw[p3 as usize] = iw[p1 as usize];
                // Add new element, me, to front of list.
                iw[p1 as usize] = me;
                // Store the new length of the list in Len[i].
                len[i as usize] = pn - p1 + 1;

                // Place in hash bucket. Save hash key of i in Last[i].

                // FIXME: this can fail if hash is negative, because the ANSI C
                // standard does not define a % b when a and/or b are negative.
                // That's why hash is defined as an unsigned int, to avoid this
                // problem.
                hash = hash % n as u32;
                debug_assert!(/*hash >= 0 &&*/ hash < n as u32);

                // If the Hhead array is not used:
                let j = head[hash as usize];
                if j <= EMPTY {
                    // Degree list is empty, hash head is flip(j).
                    next[i as usize] = flip(j);
                    head[hash as usize] = flip(i);
                } else {
                    // Degree list is not empty, use Last [Head[hash]] as hash head.
                    next[i as usize] = last[j as usize];
                    last[j as usize] = i;
                }

                // If a separate Hhead array is used:
                // 	Next [i] = Hhead[hash]
                // 	Hhead [hash] = i

                last[i as usize] = hash as i32;
            }
        }

        degree[me as usize] = degme;

        // Clear the counter array, W [...], by incrementing wflg.

        // Make sure that wflg+n does not cause integer overflow.
        lemax = max(lemax, degme);
        wflg += lemax;
        wflg = clear_flag(wflg, wbig, &mut w, n);
        // at this point, W[0..n-1] < wflg holds

        /* Supervariable Detection */

        debug1_print!("Detecting supervariables:\n");
        for pme in pme1..=pme2 {
            let mut i = iw[pme as usize];
            debug_assert!(i >= 0 && i < n);
            debug2_print!("Consider i {} nv {}\n", i, nv[i as usize]);

            if nv[i as usize] < 0 {
                // i is a principal variable in Lme.

                // Examine all hash buckets with 2 or more variables. We do
                // this by examing all unique hash keys for supervariables in
                // the pattern Lme of the current element, me.

                // Let i = head of hash bucket, and empty the hash bucket.
                debug_assert!(last[i as usize] >= 0 && last[i as usize] < n);
                hash = last[i as usize] as u32;

                // If Hhead array is not used:
                let mut j = head[hash as usize];
                if j == EMPTY {
                    // hash bucket and degree list are both empty.
                    i = EMPTY;
                } else if j < EMPTY {
                    // Degree list is empty.
                    i = flip(j);
                    head[hash as usize] = EMPTY;
                } else {
                    // Degree list is not empty, restore Last[j] of head j.
                    i = last[j as usize];
                    last[j as usize] = EMPTY;
                }

                // If separate Hhead array is used:
                // i = Hhead[hash]
                // Hhead[hash] = empty

                debug_assert!(i >= EMPTY && i < n);
                debug2_print!("----i {} hash {}\n", i, hash);

                while i != EMPTY && next[i as usize] != EMPTY {
                    // This bucket has one or more variables following i.
                    // scan all of them to see if i can absorb any entries
                    // that follow i in hash bucket. Scatter i into w.

                    let ln = len[i as usize];
                    let eln = e_len[i as usize];
                    debug_assert!(ln >= 0 && eln >= 0);
                    debug_assert!(pe[i as usize] >= 0 && pe[i as usize] < iwlen);

                    // Do not flag the first element in the list(me).
                    for p in pe[i as usize] + 1..=pe[i as usize] + ln - 1 {
                        debug_assert!(iw[p as usize] >= 0 && iw[p as usize] < n);

                        w[iw[p as usize] as usize] = wflg;
                    }

                    // Scan every other entry j following i in bucket.

                    let mut jlast = i;
                    j = next[i as usize];
                    debug_assert!(j >= EMPTY && j < n);

                    while j != EMPTY {
                        // Check if j and i have identical nonzero pattern.

                        debug3_print!("compare i {} and j {}", i, j);

                        // Check if i and j have the same Len and Elen.
                        debug_assert!(len[j as usize] >= 0 && e_len[j as usize] >= 0);
                        debug_assert!(pe[j as usize] >= 0 && pe[j as usize] < iwlen);

                        let mut ok = (len[j as usize] == ln) && (e_len[j as usize] == eln);
                        // Skip the first element in the list(me).
                        // TODO: for p := Pe[j] + 1; ok && p <= Pe[j]+ln-1; p++ {
                        for p in pe[j as usize] + 1..=pe[j as usize] + ln - 1 {
                            debug_assert!(iw[p as usize] >= 0 && iw[p as usize] < n);

                            if w[iw[p as usize] as usize] != wflg {
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            // Found it  j can be absorbed into i.
                            debug1_print!("found it! j {} => i {}\n", j, i);

                            pe[j as usize] = flip(i);
                            // Both Nv[i] and Nv[j] are negated since they
                            // are in Lme, and the absolute values of each
                            // are the number of variables in i and j:
                            nv[i as usize] += nv[j as usize];
                            nv[j as usize] = 0;
                            e_len[j as usize] = EMPTY;
                            // Delete j from hash bucket.
                            debug_assert!(j != next[j as usize]);
                            j = next[j as usize];
                            next[jlast as usize] = j;
                        } else {
                            // j cannot be absorbed into i.
                            jlast = j;
                            debug_assert!(j != next[j as usize]);
                            j = next[j as usize];
                        }
                        debug_assert!(j >= EMPTY && j < n);
                    }

                    // No more variables can be absorbed into
                    // go to next i in bucket and clear flag array.

                    wflg += 1;
                    i = next[i as usize];
                    debug_assert!(i >= EMPTY && i < n);
                }
            }
        }
        debug2_println!("detect done");

        // Restore degree lists and remove nonprincipal supervariables from element.

        let mut p = pme1;
        let nleft = n - nel;
        for pme in pme1..=pme2 {
            let i = iw[pme as usize];
            debug_assert!(i >= 0 && i < n);

            let nvi = -nv[i as usize];
            debug3_print!("Restore i {} {}\n", i, nvi);
            if nvi > 0 {
                // i is a principal variable in Lme.
                // Restore Nv[i] to signify that i is principal.
                nv[i as usize] = nvi;

                // Compute the external degree (add size of current element).

                deg = degree[i as usize] + degme - nvi;
                deg = min(deg, nleft - nvi);
                debug_assert!(implies(aggressive != 0, deg > 0) && deg >= 0 && deg < n);

                // Place the supervariable at the head of the degree list.

                inext = head[deg as usize];
                debug_assert!(inext >= EMPTY && inext < n);
                if inext != EMPTY {
                    last[inext as usize] = i;
                }
                next[i as usize] = inext;
                last[i as usize] = EMPTY;
                head[deg as usize] = i;

                // Save the new degree, and find the minimum degree.

                mindeg = min(mindeg, deg);
                degree[i as usize] = deg;

                // Place the supervariable in the element pattern.

                iw[p as usize] = i;
                p += 1;
            }
        }
        debug2_println!("restore done");

        // Finalize the new element.

        debug2_print!("ME = {} DONE\n", me);

        nv[me as usize] = nvpiv;
        // Save the length of the list for the new element me.
        len[me as usize] = p - pme1;
        if len[me as usize] == 0 {
            // There is nothing left of the current pivot element.
            // It is a root of the assembly tree.
            pe[me as usize] = EMPTY;
            w[me as usize] = 0;
        }
        if elenme != 0 {
            // Element was not constructed in place: deallocate part of
            // it since newly nonprincipal variables may have been removed.
            pfree = p;
        }

        // The new element has nvpiv pivots and the size of the contribution
        // block for a multifrontal method is degme-by-degme, not including
        // the "dense" rows/columns. If the "dense" rows/columns are included,
        // the frontal matrix is no larger than
        // (degme+ndense)-by-(degme+ndense).

        {
            let f = nvpiv;
            let r = degme + ndense;
            dmax = max(dmax, f + r);

            // Number of nonzeros in L (excluding the diagonal).
            let lnzme = f * r + (f - 1) * f / 2;
            lnz += lnzme;

            // Number of divide operations for LDL' and for LU.
            ndiv += lnzme;

            // Number of multiply-subtract pairs for LU.
            let s = f * r * r + r * (f - 1) * f + (f - 1) * f * (2 * f - 1) / 6;
            nms_lu += s;

            // Number of multiply-subtract pairs for LDL'.
            nms_ldl += (s + lnzme) / 2;
        }

        debug2_print!("finalize done nel {} n {}\n   ::::\n", nel, n);
        #[cfg(feature = "debug3")]
        for pme in pe[me as usize]..=pe[me as usize] + len[me as usize] - 1 {
            debug3_print!(" {}", iw[pme as usize]);
        }
        debug3_println!();
    }

    // Done selecting pivots.

    {
        // Count the work to factorize the ndense-by-ndense submatrix.
        let f = ndense;
        dmax = max(dmax, ndense);

        // Number of nonzeros in L (excluding the diagonal).
        let lnzme = (f - 1) * f / 2;
        lnz += lnzme;

        // Number of divide operations for LDL' and for LU.
        ndiv += lnzme;

        // Number of multiply-subtract pairs for LU.
        let s = (f - 1) * f * (2 * f - 1) / 6;
        nms_lu += s;

        // Number of multiply-subtract pairs for LDL'.
        nms_ldl += (s + lnzme) / 2;

        // Number of nz's in L (excl. diagonal).
        info.lnz = lnz;

        // Number of divide ops for LU and LDL'.
        info.n_div = ndiv;

        // Number of multiply-subtract pairs for LDL'.
        info.n_mult_subs_ldl = nms_ldl;

        // Number of multiply-subtract pairs for LU.
        info.n_mult_subs_lu = nms_lu;

        // Number of "dense" rows/columns.
        info.n_dense = ndense;

        // Largest front is dmax-by-dmax.
        info.d_max = dmax;

        // Number of garbage collections in AMD.
        info.n_cmp_a = ncmpa;

        // Successful ordering.
        info.status = Status::OK;
    }

    /* Post-ordering */

    // Variables at this point:
    //
    // Pe: holds the elimination tree. The parent of j is flip(Pe[j]),
    // or EMPTY if j is a root. The tree holds both elements and
    // non-principal (unordered) variables absorbed into them.
    // Dense variables are non-principal and unordered.
    //
    // Elen: holds the size of each element, including the diagonal part.
    // flip(Elen[e]) > 0 if e is an element. For unordered
    // variables i, Elen[i] is EMPTY.
    //
    // Nv: Nv[e] > 0 is the number of pivots represented by the element e.
    // For unordered variables i, Nv[i] is zero.
    //
    // Contents no longer needed:
    // W, Iw, Len, Degree, Head, Next, Last.
    //
    // The matrix itself has been destroyed.
    //
    // n: the size of the matrix.
    // No other scalars needed (pfree, iwlen, etc.)

    // Restore Pe.
    for i in 0..n {
        pe[i as usize] = flip(pe[i as usize]);
    }

    // Restore Elen, for output information, and for postordering.
    for i in 0..n {
        e_len[i as usize] = flip(e_len[i as usize]);
    }

    // Now the parent of j is Pe[j], or EMPTY if j is a root. Elen[e] > 0
    // is the size of element e. Elen [i] is EMPTY for unordered variable i.

    debug2_println!("\nTree:");
    #[cfg(feature = "debug1")]
    for i in 0..n {
        debug2_print!(" {} parent: {}   \n", i, pe[i as usize]);
        debug_assert!(pe[i as usize] >= EMPTY && pe[i as usize] < n);
        if nv[i as usize] > 0 {
            // This is an element.
            let e = i;
            debug2_print!(" element, size is {}", e_len[i as usize]);
            debug_assert!(e_len[e as usize] > 0);
        }
        debug2_println!();
    }
    debug2_println!("\nelements:");
    #[cfg(feature = "debug2")]
    for e in 0..n {
        if nv[e as usize] > 0 {
            debug2_print!(
                "Element e = {} size {} nv {} \n",
                e,
                e_len[e as usize],
                nv[e as usize]
            );
        }
    }
    debug3_println!("\nvariables:");
    #[cfg(feature = "debug3")]
    for i in 0..n {
        let mut cnt: i32;
        if nv[i as usize] == 0 {
            debug3_print!("i unordered: {}\n", i);
            let mut j = pe[i as usize];
            cnt = 0;
            debug3_print!("  j: {}\n", j);
            if j == EMPTY {
                debug3_println!(" i is a dense variable");
            } else {
                debug_assert!(j >= 0 && j < n);
                while nv[j as usize] == 0 {
                    debug3_print!(" j : {}\n", j);
                    j = pe[j as usize];
                    debug3_print!(" j:: {}\n", j);
                    cnt += 1;
                    if cnt > n {
                        break;
                    }
                }
                #[cfg(feature = "debug3")]
                let e = j;
                debug3_print!(" got to e: {}\n", e);
            }
        }
    }

    // Compress the paths of the variables.

    for i in 0..n {
        if nv[i as usize] == 0 {
            // i is an un-ordered row. Traverse the tree from i until
            // reaching an element, e. The element, e, was the principal
            // supervariable of i and all nodes in the path from i to when e
            // was selected as pivot.
            debug1_print!("Path compression, i unordered: {}\n", i);

            let mut j = pe[i as usize];
            debug_assert!(j >= EMPTY && j < n);
            debug3_print!(" j: {}\n", j);
            if j == EMPTY {
                // Skip a dense variable. It has no parent.
                debug3_print!("      i is a dense variable\n");
                continue;
            }

            // while (j is a variable)
            while nv[j as usize] == 0 {
                debug3_print!("  j : {}\n", j);
                j = pe[j as usize];
                debug3_print!("  j:: {}\n", j);
                debug_assert!(j >= 0 && j < n);
            }
            // Got to an element e.
            let e = j;
            debug3_print!("got to e: {}\n", e);

            // Traverse the path again from i to e, and compress the path
            // (all nodes point to e). Path compression allows this code to
            // compute in O(n) time.

            j = i;
            // while (j is a variable)
            while nv[j as usize] == 0 {
                let jnext = pe[j as usize];
                debug3_print!("j {} jnext {}\n", j, jnext);
                pe[j as usize] = e;
                j = jnext;
                debug_assert!(j >= 0 && j < n);
            }
        }
    }

    // postorder the assembly tree
    let order/*w*/ = postorder(n, pe, &nv, &e_len);

    // Compute output permutation and inverse permutation.

    // W[e] = k means that element e is the kth element in the new
    // order. e is in the range 0 to n-1, and k is in the range 0 to
    // the number of elements. Use Head for inverse order.

    for k in 0..n {
        head[k as usize] = EMPTY;
        next[k as usize] = EMPTY;
    }
    for e in 0..n {
        // let k = w[e as usize];
        let k = order[e as usize];
        debug_assert!((k == EMPTY) == (nv[e as usize] == 0));
        if k != EMPTY {
            debug_assert!(k >= 0 && k < n);
            head[k as usize] = e;
        }
    }

    // Construct output inverse permutation in Next, and permutation in Last.
    nel = 0;
    for k in 0..n {
        let e = head[k as usize];
        if e == EMPTY {
            break;
        }
        debug_assert!(e >= 0 && e < n && nv[e as usize] > 0);
        next[e as usize] = nel;
        nel += nv[e as usize];
    }
    debug_assert!(nel == n - ndense);

    // Order non-principal variables (dense, & those merged into supervar's).
    for i in 0..n {
        if nv[i as usize] == 0 {
            let e = pe[i as usize];
            debug_assert!(e >= EMPTY && e < n);
            if e != EMPTY {
                // This is an unordered variable that was merged
                // into element e via supernode detection or mass
                // elimination of i when e became the pivot element.
                // Place i in order just before e.
                debug_assert!(next[i as usize] == EMPTY && nv[e as usize] > 0);
                next[i as usize] = next[e as usize];
                next[e as usize] += 1;
            } else {
                // This is a dense unordered variable, with no parent.
                // Place it last in the output order.
                next[i as usize] = nel;
                nel += 1;
            }
        }
    }
    debug_assert!(nel == n);

    debug2_print!("\n\nPerm:\n");
    for i in 0..n {
        let k = next[i as usize];
        debug_assert!(k >= 0 && k < n);
        last[k as usize] = i;
        debug2_print!("   perm [{}] = {}\n", k, i);
    }

    (nv, next, last, e_len)
}
