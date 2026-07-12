from http import HTTPStatus
from typing import Any, cast
from urllib.parse import quote

import httpx

from ...client import AuthenticatedClient, Client
from ...types import Response, UNSET
from ... import errors

from ...models.account_history_page_response import AccountHistoryPageResponse
from ...types import UNSET, Unset
from typing import cast



def _get_kwargs(
    id: int,
    *,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> dict[str, Any]:
    

    

    params: dict[str, Any] = {}

    params["limit"] = limit

    params["before"] = before

    params["category"] = category


    params = {k: v for k, v in params.items() if v is not UNSET and v is not None}


    _kwargs: dict[str, Any] = {
        "method": "get",
        "url": "/v1/accounts/{id}/events".format(id=quote(str(id), safe=""),),
        "params": params,
    }


    return _kwargs



def _parse_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> AccountHistoryPageResponse | Any | None:
    if response.status_code == 200:
        response_200 = AccountHistoryPageResponse.from_dict(response.json())



        return response_200

    if response.status_code == 400:
        response_400 = cast(Any, None)
        return response_400

    if response.status_code == 401:
        response_401 = cast(Any, None)
        return response_401

    if response.status_code == 403:
        response_403 = cast(Any, None)
        return response_403

    if response.status_code == 500:
        response_500 = cast(Any, None)
        return response_500

    if client.raise_on_unexpected_status:
        raise errors.UnexpectedStatus(response.status_code, response.content)
    else:
        return None


def _build_response(*, client: AuthenticatedClient | Client, response: httpx.Response) -> Response[AccountHistoryPageResponse | Any]:
    return Response(
        status_code=HTTPStatus(response.status_code),
        content=response.content,
        headers=response.headers,
        parsed=_parse_response(client=client, response=response),
    )


def sync_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> Response[AccountHistoryPageResponse | Any]:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountHistoryPageResponse | Any]
     """


    kwargs = _get_kwargs(
        id=id,
limit=limit,
before=before,
category=category,

    )

    response = client.get_httpx_client().request(
        **kwargs,
    )

    return _build_response(client=client, response=response)

def sync(
    id: int,
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> AccountHistoryPageResponse | Any | None:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountHistoryPageResponse | Any
     """


    return sync_detailed(
        id=id,
client=client,
limit=limit,
before=before,
category=category,

    ).parsed

async def asyncio_detailed(
    id: int,
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> Response[AccountHistoryPageResponse | Any]:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        Response[AccountHistoryPageResponse | Any]
     """


    kwargs = _get_kwargs(
        id=id,
limit=limit,
before=before,
category=category,

    )

    response = await client.get_async_httpx_client().request(
        **kwargs
    )

    return _build_response(client=client, response=response)

async def asyncio(
    id: int,
    *,
    client: AuthenticatedClient,
    limit: int | Unset = UNSET,
    before: str | Unset = UNSET,
    category: str | Unset = UNSET,

) -> AccountHistoryPageResponse | Any | None:
    """ GET /v1/accounts/{id}/events?limit&before&category

    Args:
        id (int):
        limit (int | Unset):
        before (str | Unset):
        category (str | Unset):

    Raises:
        errors.UnexpectedStatus: If the server returns an undocumented status code and Client.raise_on_unexpected_status is True.
        httpx.TimeoutException: If the request takes longer than Client.timeout.

    Returns:
        AccountHistoryPageResponse | Any
     """


    return (await asyncio_detailed(
        id=id,
client=client,
limit=limit,
before=before,
category=category,

    )).parsed
