use anchor_lang::prelude::*;
use anchor_lang::solana_program::{clock, program_option::COption};
use anchor_spl::token::{self, TokenAccount, Token, Mint};
use std::convert::Into;
use std::convert::TryInto;

declare_id!("6CFvHBzhteMDyjUyqwvpd8qVshsfByXuSysGK9pNq5yu");

pub fn update_rewards(
    user: Option<&mut Box<Account<User>>>,
) -> Result<()> {
    if let Some(u) = user {
        let clock = clock::Clock::get().unwrap();
        let last_time_reward_applicable = clock.unix_timestamp;

        if u.balance_staked > 1100_000_000_000 {
            // u.reward_a = u.reward_a.checked_add(
            //         ((last_time_reward_applicable as u64)
            //                                 .checked_sub(u.last_update_time as u64)
            //                                 .unwrap())
            //                 .checked_mul(u.reward_a_rate as u64)
            //                 .unwrap())
            //         .unwrap();
            u.reward_b = u.reward_b.checked_add(
                    ((last_time_reward_applicable as u64)
                                            .checked_sub(u.last_update_time as u64)
                                            .unwrap())
                            .checked_mul(u.reward_b_rate as u64)
                            .unwrap())
                    .unwrap();

            // u.reward_a_rate = u.balance_staked
            //                     .checked_mul(1_000_000_000).unwrap()
            //                     .checked_mul(12).unwrap()
            //                     .checked_div(100 * 365 * 24 * 60 * 60).unwrap();
            u.reward_b_rate = u.balance_staked
                                .checked_div(365 * 24 * 60 * 60).unwrap();

            u.last_update_time = last_time_reward_applicable as u64;
        }
    }
    
    Ok(())
}

#[program]
pub mod spl_staking {
    use super::*;
    pub fn initialize(
        ctx: Context<Initialize>,
        pool_nonce: u8,
        ) -> ProgramResult {

        let pool = &mut ctx.accounts.pool;

        pool.authority = ctx.accounts.authority.key();
        pool.nonce = pool_nonce;
        pool.paused = false;
        pool.staking_mint = ctx.accounts.staking_mint.key();
        pool.staking_vault = ctx.accounts.staking_vault.key();
        pool.reward_a_mint = ctx.accounts.reward_a_mint.key();
        pool.reward_a_vault = ctx.accounts.reward_a_vault.key();
        pool.user_stake_count = 0;
        
        Ok(())
    }

    pub fn create_user(ctx: Context<CreateUser>, nonce: u8) -> Result<()> {
        let user = &mut ctx.accounts.user;
        user.pool = *ctx.accounts.pool.to_account_info().key;
        user.owner = *ctx.accounts.owner.key;

        let current_time: u64 = clock::Clock::get().unwrap().unix_timestamp.try_into().unwrap();

        user.stake_time = current_time;
        user.last_update_time = current_time;

        user.reward_a_rate = 0;
        user.reward_b_rate = 0;
        user.reward_a = 0;
        user.reward_b = 0;

        user.balance_staked = 0;
        user.nonce = nonce;

        let pool = &mut ctx.accounts.pool;
        pool.user_stake_count = pool.user_stake_count.checked_add(1).unwrap();

        Ok(())
    }

    pub fn pause(ctx: Context<Pause>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.paused = true;

        Ok(())
    }

    pub fn unpause(ctx: Context<Unpause>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.paused = false;

        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        if amount == 0 {
            return Err(ErrorCode::AmountMustBeGreaterThanZero.into());
        }

        let pool = &mut ctx.accounts.pool;
        if pool.paused {
            return Err(ErrorCode::PoolPaused.into());
        }

        ctx.accounts.user.balance_staked = ctx.accounts.user.balance_staked.checked_add(amount).unwrap();

        let user_opt = Some(&mut ctx.accounts.user);
        update_rewards(
            user_opt,
        )
        .unwrap();
        

        let current_time: u64 = clock::Clock::get().unwrap().unix_timestamp.try_into().unwrap();
        ctx.accounts.user.stake_time = current_time;

        // Transfer tokens into the stake vault.
        {
            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.stake_from_account.to_account_info(),
                    to: ctx.accounts.staking_vault.to_account_info(),
                    authority: ctx.accounts.owner.to_account_info(), //todo use user account as signer
                },
            );
            token::transfer(cpi_ctx, amount)?;
        }

        Ok(())
    }

    pub fn unstake(ctx: Context<Stake>, spt_amount: u64) -> Result<()> {
        if spt_amount == 0 {
            return Err(ErrorCode::AmountMustBeGreaterThanZero.into());
        }
        
        if ctx.accounts.user.balance_staked < spt_amount {
            return Err(ErrorCode::InsufficientFundUnstake.into());
        }

        let current_time: u64 = clock::Clock::get().unwrap().unix_timestamp.try_into().unwrap();
        if current_time < ctx.accounts.user.stake_time.checked_add(30 * 24 * 60 * 60).unwrap() {
            return Err(ErrorCode::NotEnoughUnstakePeriod.into());
        }

        ctx.accounts.user.balance_staked = ctx.accounts.user.balance_staked.checked_sub(spt_amount).unwrap();
        
        let user_opt = Some(&mut ctx.accounts.user);
        update_rewards(
            user_opt,
        )
        .unwrap();

        // Transfer tokens from the pool vault to user vault.
        {
            let seeds = &[
                ctx.accounts.pool.to_account_info().key.as_ref(),
                &[ctx.accounts.pool.nonce],
            ];
            let pool_signer = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.staking_vault.to_account_info(),
                    to: ctx.accounts.stake_from_account.to_account_info(),
                    authority: ctx.accounts.pool_signer.to_account_info(),
                },
                pool_signer,
            );
            token::transfer(cpi_ctx, spt_amount.try_into().unwrap())?;
        }

        Ok(())
    }

    pub fn withdraw(ctx: Context<Withdraw>, spt_amount: u64) -> Result<()> {
        
        // Transfer tokens from the pool vault to user vault.
        {
            let seeds = &[
                ctx.accounts.pool.to_account_info().key.as_ref(),
                &[ctx.accounts.pool.nonce],
            ];
            let pool_signer = &[&seeds[..]];

            let cpi_ctx = CpiContext::new_with_signer(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.staking_vault.to_account_info(),
                    to: ctx.accounts.withdraw_account.to_account_info(),
                    authority: ctx.accounts.pool_signer.to_account_info(),
                },
                pool_signer,
            );
            token::transfer(cpi_ctx, spt_amount.try_into().unwrap())?;
        }

        Ok(())
    }

    pub fn authorize_funder(ctx: Context<FunderChange>, funder_to_add: Pubkey) -> Result<()> {
        if funder_to_add == ctx.accounts.pool.authority.key() {
            return Err(ErrorCode::FunderAlreadyAuthorized.into());
        }
        let funders = &mut ctx.accounts.pool.funders;
        if funders.iter().any(|x| *x == funder_to_add) {
            return Err(ErrorCode::FunderAlreadyAuthorized.into());
        }
        let default_pubkey = Pubkey::default();
        if let Some(idx) = funders.iter().position(|x| *x == default_pubkey) {
            funders[idx] = funder_to_add;
        } else {
            return Err(ErrorCode::MaxFunders.into());
        }
        Ok(())
    }

    pub fn deauthorize_funder(ctx: Context<FunderChange>, funder_to_remove: Pubkey) -> Result<()> {
        if funder_to_remove == ctx.accounts.pool.authority.key() {
            return Err(ErrorCode::CannotDeauthorizePoolAuthority.into());
        }
        let funders = &mut ctx.accounts.pool.funders;
        if let Some(idx) = funders.iter().position(|x| *x == funder_to_remove) {
            funders[idx] = Pubkey::default();
        } else {
            return Err(ErrorCode::CannotDeauthorizeMissingAuthority.into());
        }
        Ok(())
    }

    pub fn fund_staking(ctx: Context<FundStaking>, amount: u64) -> Result<()> {

        if amount > 0 {
            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.from.to_account_info(),
                    to: ctx.accounts.staking_vault.to_account_info(),
                    authority: ctx.accounts.funder.to_account_info(),
                },
            );

            token::transfer(cpi_ctx, amount)?;
        }

        Ok(())
    }

    pub fn fund(ctx: Context<Fund>, amount_a: u64) -> Result<()> {

        // Transfer reward A tokens into the A vault.
        if amount_a > 0 {
            let cpi_ctx = CpiContext::new(
                ctx.accounts.token_program.to_account_info(),
                token::Transfer {
                    from: ctx.accounts.from_a.to_account_info(),
                    to: ctx.accounts.reward_a_vault.to_account_info(),
                    authority: ctx.accounts.funder.to_account_info(),
                },
            );

            token::transfer(cpi_ctx, amount_a)?;
        }

        Ok(())
    }

    pub fn claim(ctx: Context<ClaimReward>) -> Result<()> {
        let user_opt = Some(&mut ctx.accounts.user);
        update_rewards(
            user_opt,
        )
        .unwrap();

        let seeds = &[
            ctx.accounts.pool.to_account_info().key.as_ref(),
            &[ctx.accounts.pool.nonce],
        ];
        let pool_signer = &[&seeds[..]];

        if ctx.accounts.user.reward_a > 0 {
            let mut reward_amount = ctx.accounts.user.reward_a;
            let vault_balance = ctx.accounts.reward_a_vault.amount;

            ctx.accounts.user.reward_a = 0;
            if vault_balance < reward_amount {
                reward_amount = vault_balance;
            }

            if reward_amount > 0 {
                let cpi_ctx = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.reward_a_vault.to_account_info(),
                        to: ctx.accounts.reward_a_account.to_account_info(),
                        authority: ctx.accounts.pool_signer.to_account_info(),
                    },
                    pool_signer,
                );
                token::transfer(cpi_ctx, reward_amount)?;
            }
        }

        if ctx.accounts.user.reward_b > 0 {
            let mut reward_amount = ctx.accounts.user.reward_b;
            let vault_balance = ctx.accounts.staking_vault.amount;

            ctx.accounts.user.reward_b = 0;
            if vault_balance < reward_amount {
                reward_amount = vault_balance;
            }

            if reward_amount > 0 {
                let cpi_ctx = CpiContext::new_with_signer(
                    ctx.accounts.token_program.to_account_info(),
                    token::Transfer {
                        from: ctx.accounts.staking_vault.to_account_info(),
                        to: ctx.accounts.reward_b_account.to_account_info(),
                        authority: ctx.accounts.pool_signer.to_account_info(),
                    },
                    pool_signer,
                );
                token::transfer(cpi_ctx, reward_amount)?;
            }
        }

        Ok(())
    }

    pub fn close_user(ctx: Context<CloseUser>) -> Result<()> {
        let pool = &mut ctx.accounts.pool;
        pool.user_stake_count = pool.user_stake_count.checked_sub(1).unwrap();
        Ok(())
    }

    pub fn close_pool<'info>(ctx: Context<ClosePool>) -> Result<()> {
        let pool = &ctx.accounts.pool;
        
        let signer_seeds = &[pool.to_account_info().key.as_ref(), &[ctx.accounts.pool.nonce]];
        
        //instead of closing these vaults, we could technically just 
        //set_authority on them. it's not very ata clean, but it'd work
        //if size of tx is an issue, thats an approach

        //close staking vault
        let ix = spl_token::instruction::transfer(
            &spl_token::ID,
            ctx.accounts.staking_vault.to_account_info().key,
            ctx.accounts.staking_refundee.to_account_info().key,
            ctx.accounts.pool_signer.key,
            &[ctx.accounts.pool_signer.key],
            ctx.accounts.staking_vault.amount,
        )?;
        solana_program::program::invoke_signed(
            &ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.staking_vault.to_account_info(),
                ctx.accounts.staking_refundee.to_account_info(),
                ctx.accounts.pool_signer.to_account_info(),
            ],
            &[signer_seeds],
        )?;
        let ix = spl_token::instruction::close_account(
            &spl_token::ID,
            ctx.accounts.staking_vault.to_account_info().key,
            ctx.accounts.refundee.key,
            ctx.accounts.pool_signer.key,
            &[ctx.accounts.pool_signer.key],
        )?;
        solana_program::program::invoke_signed(
            &ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.staking_vault.to_account_info(),
                ctx.accounts.refundee.to_account_info(),
                ctx.accounts.pool_signer.to_account_info(),
            ],
            &[signer_seeds],
        )?;
        
        //close token a vault
        let ix = spl_token::instruction::transfer(
            &spl_token::ID,
            ctx.accounts.reward_a_vault.to_account_info().key,
            ctx.accounts.reward_a_refundee.to_account_info().key,
            ctx.accounts.pool_signer.key,
            &[ctx.accounts.pool_signer.key],
            ctx.accounts.reward_a_vault.amount,
        )?;
        solana_program::program::invoke_signed(
            &ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.reward_a_vault.to_account_info(),
                ctx.accounts.reward_a_refundee.to_account_info(),
                ctx.accounts.pool_signer.to_account_info(),
            ],
            &[signer_seeds],
        )?;
        let ix = spl_token::instruction::close_account(
            &spl_token::ID,
            ctx.accounts.reward_a_vault.to_account_info().key,
            ctx.accounts.refundee.key,
            ctx.accounts.pool_signer.key,
            &[ctx.accounts.pool_signer.key],
        )?;
        solana_program::program::invoke_signed(
            &ix,
            &[
                ctx.accounts.token_program.to_account_info(),
                ctx.accounts.reward_a_vault.to_account_info(),
                ctx.accounts.refundee.to_account_info(),
                ctx.accounts.pool_signer.to_account_info(),
            ],
            &[signer_seeds],
        )?;
        
        Ok(())
    }
}

#[derive(Accounts)]
#[instruction(pool_nonce: u8)]
pub struct Initialize<'info> {
    authority: UncheckedAccount<'info>,

    staking_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = staking_vault.mint == staking_mint.key(),
        constraint = staking_vault.owner == pool_signer.key(),
        //strangely, spl maintains this on owner reassignment for non-native accounts
        //we don't want to be given an account that someone else could close when empty
        //because in our "pool close" operation we want to assert it is still open
        constraint = staking_vault.close_authority == COption::None,
    )]
    staking_vault: Box<Account<'info, TokenAccount>>,

    reward_a_mint: Box<Account<'info, Mint>>,
    #[account(
        constraint = reward_a_vault.mint == reward_a_mint.key(),
        constraint = reward_a_vault.owner == pool_signer.key(),
        constraint = reward_a_vault.close_authority == COption::None,
    )]
    reward_a_vault: Box<Account<'info, TokenAccount>>,

    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool_nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    #[account(
        zero,
    )]
    pool: Box<Account<'info, Pool>>,
    
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
#[instruction(nonce: u8)]
pub struct CreateUser<'info> {
    // Stake instance.
    #[account(
        mut,
        constraint = !pool.paused,
    )]
    pool: Box<Account<'info, Pool>>,
    // Member.
    #[account(
        init,
        payer = owner,
        seeds = [
            owner.key.as_ref(), 
            pool.to_account_info().key.as_ref()
        ],
        bump = nonce,
    )]
    user: Box<Account<'info, User>>,
    owner: Signer<'info>,
    // Misc.
    system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Pause<'info> {
    #[account(
        mut, 
        has_one = authority,
        constraint = !pool.paused,
    )]
    pool: Box<Account<'info, Pool>>,
    authority: Signer<'info>,

    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Unpause<'info> {
    #[account(
        mut, 
        has_one = authority,
        constraint = pool.paused,
    )]
    pool: Box<Account<'info, Pool>>,
    authority: Signer<'info>,

    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = staking_vault,
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        constraint = staking_vault.owner == *pool_signer.key,
    )]
    staking_vault: Box<Account<'info, TokenAccount>>,

    // User.
    #[account(
        mut, 
        has_one = owner, 
        has_one = pool,
        seeds = [
            owner.key.as_ref(), 
            pool.to_account_info().key.as_ref()
        ],
        bump = user.nonce,
    )]
    user: Box<Account<'info, User>>,
    owner: Signer<'info>,
    #[account(mut)]
    stake_from_account: Box<Account<'info, TokenAccount>>,

    // Program signers.
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    // Misc.
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Withdraw<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = staking_vault,
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        constraint = staking_vault.owner == *pool_signer.key,
    )]
    staking_vault: Box<Account<'info, TokenAccount>>,

    owner: Signer<'info>,
    #[account(
        mut,
    )]
    withdraw_account: Box<Account<'info, TokenAccount>>,

    // Program signers.
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    // Misc.
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct FunderChange<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = authority,
    )]
    pool: Box<Account<'info, Pool>>,
    authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct Fund<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = reward_a_vault,
        constraint = !pool.paused,
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(mut)]
    reward_a_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        //require signed funder auth - otherwise constant micro fund could hold funds hostage
        constraint = funder.key() == pool.authority || pool.funders.iter().any(|x| *x == funder.key()),
    )]
    funder: Signer<'info>,
    #[account(mut)]
    from_a: Box<Account<'info, TokenAccount>>,
    
    // Program signers.
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    // Misc.
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct FundStaking<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = staking_vault,
        constraint = !pool.paused,
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(mut)]
    staking_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        //require signed funder auth - otherwise constant micro fund could hold funds hostage
        constraint = funder.key() == pool.authority || pool.funders.iter().any(|x| *x == funder.key()),
    )]
    funder: Signer<'info>,
    #[account(mut)]
    from: Box<Account<'info, TokenAccount>>,
    
    // Program signers.
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    // Misc.
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimReward<'info> {
    // Global accounts for the staking instance.
    #[account(
        mut, 
        has_one = staking_vault,
        has_one = reward_a_vault,
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(mut)]
    staking_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reward_a_vault: Box<Account<'info, TokenAccount>>,

    // User.
    #[account(
        mut,
        has_one = owner,
        has_one = pool,
        seeds = [
            owner.to_account_info().key.as_ref(),
            pool.to_account_info().key.as_ref()
        ],
        bump = user.nonce,
    )]
    user: Box<Account<'info, User>>,
    owner: Signer<'info>,
    #[account(mut)]
    reward_a_account: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reward_b_account: Box<Account<'info, TokenAccount>>,

    // Program signers.
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,

    // Misc.
    token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct CloseUser<'info> {
    #[account(
        mut, 
    )]
    pool: Box<Account<'info, Pool>>,
    #[account(
        mut,
        close = owner,
        has_one = owner,
        has_one = pool,
        seeds = [
            owner.to_account_info().key.as_ref(),
            pool.to_account_info().key.as_ref()
        ],
        bump = user.nonce,
        constraint = user.balance_staked == 0,
        constraint = user.reward_a == 0,
        constraint = user.reward_b == 0,
    )]
    user: Account<'info, User>,
    owner: Signer<'info>,
}

#[derive(Accounts)]
pub struct ClosePool<'info> {
    #[account(mut)]
    refundee: UncheckedAccount<'info>,
    #[account(mut)]
    staking_refundee: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reward_a_refundee: Box<Account<'info, TokenAccount>>,
    #[account(
        mut,
        close = refundee,
        has_one = authority,
        has_one = staking_vault,
        has_one = reward_a_vault,
        constraint = pool.paused,
        constraint = pool.user_stake_count == 0,
    )]
    pool: Account<'info, Pool>,
    authority: Signer<'info>,
    #[account(mut,
        constraint = staking_vault.amount == 0,
    )]
    staking_vault: Box<Account<'info, TokenAccount>>,
    #[account(mut)]
    reward_a_vault: Box<Account<'info, TokenAccount>>,
    #[account(
        seeds = [
            pool.to_account_info().key.as_ref()
        ],
        bump = pool.nonce,
    )]
    pool_signer: UncheckedAccount<'info>,
    token_program: Program<'info, Token>,
}

#[account]
pub struct Pool {
    /// Priviledged account.
    pub authority: Pubkey,
    /// Nonce to derive the program-derived address owning the vaults.
    pub nonce: u8,
    /// Paused state of the program
    pub paused: bool,
    /// Mint of the token that can be staked.
    pub staking_mint: Pubkey,
    /// Vault to store staked tokens.
    pub staking_vault: Pubkey,
    /// Mint of the reward A token.
    pub reward_a_mint: Pubkey,
    /// Vault to store reward A tokens.
    pub reward_a_vault: Pubkey,
    /// Users staked
    pub user_stake_count: u32,
    /// authorized funders
    /// [] because short size, fixed account size, and ease of use on 
    /// client due to auto generated account size property
    pub funders: [Pubkey; 5],
}

#[account]
#[derive(Default)]
pub struct User {
    /// Pool the this user belongs to.
    pub pool: Pubkey,
    /// The owner of this account.
    pub owner: Pubkey,
    /// The amount of token A claimed.
    pub reward_a: u64,
    /// The amount of token B claimed.
    pub reward_b: u64,
    /// The amount of token A pending claim.
    pub reward_a_rate: u64,
    /// The amount of token B pending claim.
    pub reward_b_rate: u64,
    /// The amount staked.
    pub balance_staked: u64,
    /// The last time reward states were updated.
    pub last_update_time: u64,
    /// The last stake time
    pub stake_time: u64,
    /// Signer nonce.
    pub nonce: u8,
}

#[error]
pub enum ErrorCode {
    #[msg("Insufficient funds to unstake.")]
    InsufficientFundUnstake,
    #[msg("Amount must be greater than zero.")]
    AmountMustBeGreaterThanZero,
    #[msg("Reward B cannot be funded - pool is single stake.")]
    SingleStakeTokenBCannotBeFunded,
    #[msg("Pool is paused.")]
    PoolPaused,
    #[msg("Duration cannot be shorter than one day.")]
    DurationTooShort,
    #[msg("Provided funder is already authorized to fund.")]
    FunderAlreadyAuthorized,
    #[msg("Maximum funders already authorized.")]
    MaxFunders,
    #[msg("Cannot deauthorize the primary pool authority.")]
    CannotDeauthorizePoolAuthority,
    #[msg("Authority not found for deauthorization.")]
    CannotDeauthorizeMissingAuthority,
    #[msg("Can unstake only after 30 days more.")]
    NotEnoughUnstakePeriod,
}
