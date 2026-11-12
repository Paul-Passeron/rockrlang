/* Rockr programming language
Copyright (C) 2026  NoRezap

This program is free software: you can redistribute it and/or modify
it under the terms of the GNU General Public License as published by
the Free Software Foundation, either version 3 of the License, or
(at your option) any later version.

This program is distributed in the hope that it will be useful,
but WITHOUT ANY WARRANTY; without even the implied warranty of
MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
GNU General Public License for more details.

You should have received a copy of the GNU General Public License
along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */

use std::{
    convert::Infallible,
    ops::{ControlFlow, FromResidual, Residual, Try},
};

#[derive(Debug)]
pub enum ConstraintSolveResult<Err> {
    Solved,
    Pending,
    Error(Err),
}

pub enum ConstraintSolveResultResidual<Err> {
    Pending,
    Error(Err),
}

impl<Err> FromResidual<ConstraintSolveResultResidual<Err>>
    for ConstraintSolveResult<Err>
{
    fn from_residual(residual: ConstraintSolveResultResidual<Err>) -> Self {
        match residual {
            ConstraintSolveResultResidual::Pending => Self::Pending,
            ConstraintSolveResultResidual::Error(err) => Self::Error(err),
        }
    }
}

impl<Err> FromResidual<Result<Infallible, Err>> for ConstraintSolveResult<Err> {
    fn from_residual(residual: Result<Infallible, Err>) -> Self {
        let Err(err) = residual;
        Self::Error(err)
    }
}

impl<Err> From<Result<(), Err>> for ConstraintSolveResult<Err> {
    fn from(res: Result<(), Err>) -> Self {
        res.map_or_else(Self::Error, |()| Self::Solved)
    }
}

impl<Err> Residual<()> for ConstraintSolveResultResidual<Err> {
    type TryType = ConstraintSolveResult<Err>;
}

impl<Err> Try for ConstraintSolveResult<Err> {
    type Output = ();

    type Residual = ConstraintSolveResultResidual<Err>;

    fn from_output((): Self::Output) -> Self {
        Self::Solved
    }

    fn branch(self) -> ControlFlow<Self::Residual, Self::Output> {
        match self {
            Self::Solved => ControlFlow::Continue(()),
            Self::Pending => ControlFlow::Break(ConstraintSolveResultResidual::Pending),
            Self::Error(err) => {
                ControlFlow::Break(ConstraintSolveResultResidual::Error(err))
            }
        }
    }
}
