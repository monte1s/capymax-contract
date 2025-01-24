use anchor_lang::prelude::*;
use anchor_spl::token::{self, Token, TokenAccount, Mint, Transfer};

declare_id!("6CFvHBzhteMDyjUyqwvpd8qVshsfByXuSysGK9pNq5yu");

#[program]
pub mod staking_contract {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, reward_rate: u64) -> Result<()> {
        let staking_account = &mut ctx.accounts.staking_account;
        staking_account.admin = *ctx.accounts.admin.key;
        staking_account.reward_rate = reward_rate;
        staking_account.total_staked = 0;
        Ok(())
    }

    pub fn stake(ctx: Context<Stake>, amount: u64) -> Result<()> {
        let staking_account = &mut ctx.accounts.staking_account;
        let user_stake = &mut ctx.accounts.user_stake;

        token::transfer(ctx.accounts.into_transfer_to_staking_context(), amount)?;

        user_stake.amount += amount;
        user_stake.reward_debt = (user_stake.amount * staking_account.reward_rate) / 1_000_000;
        staking_account.total_staked += amount;

        Ok(())
    }

    pub fn unstake(ctx: Context<Unstake>, amount: u64) -> Result<()> {
        let staking_account = &mut ctx.accounts.staking_account;
        let user_stake = &mut ctx.accounts.user_stake;

        require!(user_stake.amount >= amount, CustomError::InsufficientStakedAmount);

        token::transfer(ctx.accounts.into_transfer_to_user_context(), amount)?;

        user_stake.amount -= amount;
        user_stake.reward_debt = (user_stake.amount * staking_account.reward_rate) / 1_000_000;
        staking_account.total_staked -= amount;

        Ok(())
    }

    pub fn claim_rewards(ctx: Context<ClaimRewards>) -> Result<()> {
        let staking_account = &mut ctx.accounts.staking_account;
        let user_stake = &mut ctx.accounts.user_stake;

        let pending_rewards = (user_stake.amount * staking_account.reward_rate) / 1_000_000 - user_stake.reward_debt;

        require!(pending_rewards > 0, CustomError::NoRewardsAvailable);

        token::transfer(ctx.accounts.into_transfer_rewards_context(), pending_rewards)?;

        user_stake.reward_debt += pending_rewards;

        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, payer = admin, space = 8 + 32 + 8 + 8)]
    pub staking_account: Account<'info, StakingAccount>,
    #[account(mut)]
    pub admin: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Stake<'info> {
    #[account(mut)]
    pub staking_account: Account<'info, StakingAccount>,
    #[account(mut)]
    pub user_stake: Account<'info, UserStake>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub staking_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct Unstake<'info> {
    #[account(mut)]
    pub staking_account: Account<'info, StakingAccount>,
    #[account(mut)]
    pub user_stake: Account<'info, UserStake>,
    #[account(mut)]
    pub user_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub staking_token_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[derive(Accounts)]
pub struct ClaimRewards<'info> {
    #[account(mut)]
    pub staking_account: Account<'info, StakingAccount>,
    #[account(mut)]
    pub user_stake: Account<'info, UserStake>,
    #[account(mut)]
    pub reward_token_account: Account<'info, TokenAccount>,
    #[account(mut)]
    pub user_reward_account: Account<'info, TokenAccount>,
    pub token_program: Program<'info, Token>,
}

#[account]
pub struct StakingAccount {
    pub admin: Pubkey,
    pub reward_rate: u64,
    pub total_staked: u64,
}

#[account]
pub struct UserStake {
    pub amount: u64,
    pub reward_debt: u64,
}

impl<'info> Stake<'info> {
    fn into_transfer_to_staking_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.user_token_account.to_account_info().clone(),
            to: self.staking_token_account.to_account_info().clone(),
            authority: self.user_stake.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

impl<'info> Unstake<'info> {
    fn into_transfer_to_user_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.staking_token_account.to_account_info().clone(),
            to: self.user_token_account.to_account_info().clone(),
            authority: self.staking_account.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

impl<'info> ClaimRewards<'info> {
    fn into_transfer_rewards_context(&self) -> CpiContext<'_, '_, '_, 'info, Transfer<'info>> {
        let cpi_accounts = Transfer {
            from: self.reward_token_account.to_account_info().clone(),
            to: self.user_reward_account.to_account_info().clone(),
            authority: self.staking_account.to_account_info().clone(),
        };
        CpiContext::new(self.token_program.to_account_info().clone(), cpi_accounts)
    }
}

#[error_code]
pub enum CustomError {
    #[msg("Insufficient staked amount.")]
    InsufficientStakedAmount,
    #[msg("No rewards available to claim.")]
    NoRewardsAvailable,
}
