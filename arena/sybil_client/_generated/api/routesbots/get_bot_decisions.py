from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.bot_decision_feed_response import BotDecisionFeedResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    *,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["trader"] = trader


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/bots/decisions",
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> BotDecisionFeedResponse | None:
    if response.status_code == 200:
        response_200 = BotDecisionFeedResponse.from_dict(response.json())



        return response_200

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[BotDecisionFeedResponse]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,

) -> Response[BotDecisionFeedResponse]:
    """ GET /v1/bots/decisions

     Native arena / bot analytics feed. Public (unauthenticated) read route.

    Args:
        limit (int | Unset):
        trader (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BotDecisionFeedResponse]
     """


    kwargs = _get_kwargs(
        limit=limit,
trader=trader,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,

) -> BotDecisionFeedResponse | None:
    """ GET /v1/bots/decisions

     Native arena / bot analytics feed. Public (unauthenticated) read route.

    Args:
        limit (int | Unset):
        trader (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BotDecisionFeedResponse
     """


    return sync_detailed(
        client=client,
limit=limit,
trader=trader,

    ).parsed

async def asyncio_detailed(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,

) -> Response[BotDecisionFeedResponse]:
    """ GET /v1/bots/decisions

     Native arena / bot analytics feed. Public (unauthenticated) read route.

    Args:
        limit (int | Unset):
        trader (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[BotDecisionFeedResponse]
     """


    kwargs = _get_kwargs(
        limit=limit,
trader=trader,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    *,
    client: AuthenticatedClient | Client,
    limit: int | Unset = UNSET,
    trader: str | Unset = UNSET,

) -> BotDecisionFeedResponse | None:
    """ GET /v1/bots/decisions

     Native arena / bot analytics feed. Public (unauthenticated) read route.

    Args:
        limit (int | Unset):
        trader (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        BotDecisionFeedResponse
     """


    return (await asyncio_detailed(
        client=client,
limit=limit,
trader=trader,

    )).parsed
